#!/bin/sh
# taskq.md § 8 step 5 (#1329): cargo runner on macOS. Signs the binary cargo starts and the taskq binary of the same
# target directory (integration tests spawn it) with `taskq sign-local`, which reuses platform::codesign, then execs.
set -eu
dir=$(dirname "$1")
[ "$(basename "$dir")" = deps ] && dir=$(dirname "$dir")
signer=$dir/taskq
if [ -x "$signer" ]; then
  if [ "$1" -ef "$signer" ]; then "$signer" sign-local "$1"; else "$signer" sign-local "$1" "$signer"; fi
else
  echo "signing: no $signer to sign $1; it stays ad-hoc" >&2
fi
exec "$@"
