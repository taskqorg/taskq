#!/bin/sh
# §8: upload this attested OS independently; serialize the complete index transaction.
set -eu
tag=$1 d=$2 ver=${1#v} r=$GITHUB_REPOSITORY
here=$(cd "$(dirname "$0")" && pwd)
idx=$(mktemp -d)
lock="taskq-release-locks/$tag"
owned=0
cleanup() {
  if [ "$owned" = 1 ]; then gh api --method DELETE "repos/$r/git/refs/$lock" >/dev/null; fi
  rm -rf "$idx"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
# Atomic create is exclusive. Unknown/failed acquisition never permits an index write.
# A killed runner leaves the lock held; never infer release from age or steal it.
tries=0
until gh api --method POST "repos/$r/git/refs" -f "ref=refs/$lock" -f "sha=$GITHUB_SHA" > /dev/null 2>&1; do
  tries=$((tries + 1))
  [ "$tries" -lt 60 ] || { echo "::error::$tag publication lock unavailable; verify prior publisher ended before removing refs/$lock" >&2; exit 1; }
  sleep "${LOCK_WAIT:-2}"
done
owned=1
gh release upload "$tag" "$d"/taskq-"$ver"-* --repo "$r" --clobber
# Read errors must not masquerade as an empty index. The first publisher has no sums.
assets=$(gh release view "$tag" --repo "$r" --json assets --jq '.assets[].name')
if printf '%s\n' "$assets" | grep -qx SHA256SUMS; then
  gh release download "$tag" --repo "$r" --pattern SHA256SUMS --dir "$idx"
else
  : > "$idx/SHA256SUMS"
fi
cp "$d"/taskq-"$ver"-* "$idx/"
python3 "$here/release-index.py" "$ver" "$idx"
gh release upload "$tag" "$idx/SHA256SUMS" --repo "$r" --clobber
gh release upload "$tag" "$idx/latest.json" --repo "$r" --clobber
gh release edit "$tag" --repo "$r" --draft=false
# Release only after every publication write has completed successfully.
gh api --method DELETE "repos/$r/git/refs/$lock" > /dev/null
owned=0
