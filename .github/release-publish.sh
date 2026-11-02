#!/bin/sh
# release.yml publish jobs (taskq.md § 8 item 1): adds one OS's files in <dir> to the release <tag> and publishes it.
# Each OS job runs this as soon as its build is attested, so linux and windows never wait for each other or macos.
# Order per round: own files (once), then SHA256SUMS, then latest.json, so a host never reads an entry without both.
# The index is read-merge-written; a concurrent OS job can overwrite it, so after SETTLE seconds this rereads both
# files and merges again until they list every own line. The first job to finish un-drafts the release.
# ponytail: no lock, settle-and-verify; fails after 6 rounds; a ref lock if the index ever needs strict ordering.
# Usage: release-publish.sh <tag> <dir>
set -eu
tag=$1 d=$2 ver=${1#v} r=$GITHUB_REPOSITORY settle=${SETTLE:-60}
here=$(cd "$(dirname "$0")" && pwd)
own=$(cd "$d" && shasum -a 256 taskq-"$ver"-*)
[ -n "$own" ] || { echo "::error::no taskq-$ver-* files in $d" >&2; exit 1; }
gh release upload "$tag" "$d"/taskq-"$ver"-* --repo "$r" --clobber
idx=$(mktemp -d)
trap 'rm -rf "$idx"' EXIT
listed() {
  rm -f "$idx"/SHA256SUMS "$idx"/latest.json
  gh release download "$tag" --repo "$r" --pattern SHA256SUMS --pattern latest.json --dir "$idx" 2>/dev/null || true
  [ -f "$idx"/SHA256SUMS ] && [ -f "$idx"/latest.json ] || return 1
  printf '%s\n' "$own" | while read -r sum name; do
    grep -qx "$sum  $name" "$idx"/SHA256SUMS && grep -q "\"$name\"" "$idx"/latest.json || return 1
  done
}
round=0
until listed; do
  round=$((round + 1))
  [ "$round" -le 6 ] || { echo "::error::$tag index lost this OS's lines 6 times; release stays as is" >&2; exit 1; }
  [ -f "$idx"/SHA256SUMS ] || : > "$idx"/SHA256SUMS
  printf '%s\n' "$own" | sed 's/ \*/  /' >> "$idx"/SHA256SUMS
  python3 "$here"/release-index.py "$ver" "$idx"
  gh release upload "$tag" "$idx"/SHA256SUMS --repo "$r" --clobber
  gh release upload "$tag" "$idx"/latest.json --repo "$r" --clobber
  sleep "$settle"
done
gh release edit "$tag" --repo "$r" --draft=false
