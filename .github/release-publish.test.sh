#!/bin/sh
# Chain check for the release.yml publish jobs (taskq.md § 8 item 1, R12): each OS adds its files to the shared
# release index on its own; an index overwritten by a concurrent OS job is merged again until it lists every own
# line; an index lost every round fails without un-drafting. gh is a stub over a local store; nothing leaves the runner.
set -eu
src=$(cd "$(dirname "$0")" && pwd)/release-publish.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cd "$t"
mkdir bin store
cat > bin/gh <<'SH'
#!/bin/sh
# Stub: upload copies into store/; after a latest.json upload, race/ (if present) overwrites the index like a
# concurrent OS job; RACE=always keeps it. download copies the patterns present, else fails. edit is logged.
t=$(cd "$(dirname "$0")/.." && pwd)
op=$2; shift 3
case $op in
  upload)
    for f; do case $f in --*|o/r) ;; *) cp "$f" "$t/store/";; esac; done
    for f; do case $f in */latest.json) [ -d "$t/race" ] && cp "$t"/race/* "$t/store/" && { [ "${RACE:-}" = always ] || rm -rf "$t/race"; };; esac; done;;
  download)
    dir=; got=0; prev=
    for a; do [ "$prev" = --dir ] && dir=$a; prev=$a; done
    for f in SHA256SUMS latest.json; do [ -f "$t/store/$f" ] && cp "$t/store/$f" "$dir/" && got=1; done
    [ $got = 1 ];;
  edit) echo "edit $*" >> "$t/edits";;
  *) exit 2;;
esac
SH
chmod +x bin/gh
export PATH="$t/bin:$PATH" GITHUB_REPOSITORY=o/r SETTLE=0
fails=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fails=1; fi; }
os() { rm -rf "$1"; mkdir "$1"; echo "$1 binary" > "$1/taskq-1.2.3-$1"; }

os linux-x86_64
sh "$src" v1.2.3 linux-x86_64 > /dev/null
check 'first OS: indexed and un-drafted' \
  'grep -q " taskq-1.2.3-linux-x86_64$" store/SHA256SUMS && grep -q "\"linux-x86_64\"" store/latest.json && [ $(wc -l < edits) -eq 1 ]'

# a concurrent job that read the index before windows wrote it overwrites it with linux only
mkdir race; cp store/SHA256SUMS store/latest.json race/
os windows-x86_64
sh "$src" v1.2.3 windows-x86_64 > /dev/null
check 'overwritten index merged again' \
  '[ ! -d race ] && grep -q "taskq-1.2.3-linux-x86_64" store/latest.json && grep -q "taskq-1.2.3-windows-x86_64" store/latest.json && [ $(wc -l < store/SHA256SUMS) -eq 2 ]'

mkdir race; printf 'x  taskq-1.2.3-linux-x86_64\n' > race/SHA256SUMS; echo '{}' > race/latest.json
os macos-aarch64
rc=0; RACE=always sh "$src" v1.2.3 macos-aarch64 > /dev/null 2>&1 || rc=$?
check 'index lost every round: fails, stays as before' '[ $rc = 1 ] && [ $(wc -l < edits) -eq 2 ]'
exit $fails
