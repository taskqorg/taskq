#!/bin/sh
# Chain check for the release.yml tag job (taskq.md § 8 item 1, R12): a live push tags the next unused vX.Y.Z
# (Cargo.toml version or highest tag plus one patch, whichever is greater) on the pushed commit; a commit that
# already has a tag reuses it only while its release is absent; unreadable tags or an invalid version fail
# without a tag; a manual tag push releases that tag. gh is a stub; nothing leaves the runner.
set -eu
src=$(cd "$(dirname "$0")" && pwd)/release-tag.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cd "$t"
mkdir bin
cat > bin/gh <<'SH'
#!/bin/sh
# Stub: TAGS = "<ref> <sha>" lines or "503"; RELEASED = 1 when `release view` finds the release. Writes are logged.
case "$*" in
  *matching-refs*) [ "$TAGS" = 503 ] && { echo 'HTTP 503' >&2; exit 1; }; [ -z "$TAGS" ] || printf '%s\n' "$TAGS";;
  *git/refs*) echo "$*" >> writes;;
  'release view'*) [ "$RELEASED" = 1 ];;
  *) exit 2;;
esac
SH
chmod +x bin/gh
export PATH="$t/bin:$PATH" GITHUB_REPOSITORY=o/r GITHUB_SHA=abc GITHUB_REF_NAME=live GITHUB_REF_TYPE=branch
fails=0
# $1 name, $2 version line, $3 tags, $4 released, $5 expected exit, $6 expected output, $7 expected tag write
case_() {
  printf '%s\n' "$2" > Cargo.toml; rm -f writes; touch writes
  rc=0; out=$(TAGS=$3 RELEASED=$4 sh "$src" 2>/dev/null) || rc=$?
  w=$(sed -n 's|.*ref=refs/tags/\([^ ]*\) -f sha=abc$|\1|p' writes)
  if [ "$rc" = "$5" ] && [ "$out" = "$6" ] && [ "$w" = "$7" ]; then echo "ok   $1"
  else echo "FAIL $1: exit $rc out '$out' write '$w'; want $5 '$6' '$7'"; fails=1; fi
}
nl='
'
case_ 'no tags: Cargo version' 'version = "1.1.0"' '' 0 0 tag=v1.1.0 v1.1.0
case_ 'tag above Cargo: next patch' 'version = "1.1.0"' "refs/tags/v1.1.0 d0${nl}refs/tags/v1.1.9 d9${nl}refs/tags/v1.1.10 d10" 0 0 tag=v1.1.11 v1.1.11
case_ 'Cargo above tags: Cargo version' 'version = "1.2.0"' 'refs/tags/v1.1.10 d10' 0 0 tag=v1.2.0 v1.2.0
case_ 'other tag names ignored' 'version = "1.1.0"' "refs/tags/v1.1.3 d3${nl}refs/tags/v9.9.9-rc1 dx${nl}refs/tags/vx d4" 0 0 tag=v1.1.4 v1.1.4
case_ 'commit tagged, unreleased: reuses tag' 'version = "1.1.0"' 'refs/tags/v1.1.3 abc' 0 0 tag=v1.1.3 ''
case_ 'commit tagged, released: nothing' 'version = "1.1.0"' 'refs/tags/v1.1.3 abc' 1 0 '' ''
case_ 'tag list 503: fails' 'version = "1.1.0"' 503 0 1 '' ''
case_ 'invalid version: fails' 'version = "x"' '' 0 1 '' ''
GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v9.9.9 case_ 'manual tag push: releases it' 'version = "1.1.0"' 503 0 0 tag=v9.9.9 ''
exit $fails
