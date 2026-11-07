#!/bin/sh
# Actual release tag generator with owned offline gh/date stubs; no remote writes.
# Keep date floor, monotonic allocation, source ancestry and immutable-tag controls together.
set -eu
src=$(cd "$(dirname "$0")" && pwd)/release-tag.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cd "$t"
mkdir bin
cat > bin/gh <<'SH'
#!/bin/sh
# Stub: TAGS = "<ref> <sha>" lines or "503"; RELEASED = 1 (published) or draft when `release view` finds the release. Writes are logged.
case "$*" in
  *releases/latest*) [ "$LATEST" = 503 ] && exit 1; echo "$LATEST";;
  *compare/*) [ "$COVERAGE" = 503 ] && exit 1; echo "$COVERAGE";;
  *matching-refs*) [ "$TAGS" = 503 ] && { echo 'HTTP 503' >&2; exit 1; }; [ -z "$TAGS" ] || printf '%s\n' "$TAGS";;
  *git/refs*) echo "$*" >> writes;;
  'release view'*) case $RELEASED in 1) echo false;; draft) echo true;; *) exit 1;; esac;;
  *) exit 2;;
esac
SH
cat > bin/date <<'SH'
#!/bin/sh
# Refuse a local-time or independently formatted date request.
[ "$*" = '-u +%y%m%d' ] || exit 2
[ "$UTC_DAY" != 503 ] || exit 1
printf '%s\n' "$UTC_DAY"
SH
chmod +x bin/gh bin/date
export UTC_DAY=261107 TZ=Pacific/Kiritimati
export PATH="$t/bin:$PATH" GITHUB_REPOSITORY=o/r GITHUB_SHA=abc GITHUB_REF_NAME=live GITHUB_REF_TYPE=branch
fails=0
# $1 name, $2 version line, $3 tags, $4 released, $5 expected exit, $6 expected output, $7 expected tag write
case_() {
  printf '%s\n' "$2" > Cargo.toml; rm -f writes; touch writes
  # POSIX shells can retain assignments on function calls; isolate each case's clock.
  utc_day=$UTC_DAY; UTC_DAY=261107
  rc=0; out=$(UTC_DAY=$utc_day TAGS=$3 RELEASED=$4 sh "$src" 2>/dev/null) || rc=$?
  w=$(sed -n 's|.*ref=refs/tags/\([^ ]*\) -f sha=abc$|\1|p' writes)
  if [ "$rc" = "$5" ] && [ "$out" = "$6" ] && [ "$w" = "$7" ]; then echo "ok   $1"
  else echo "FAIL $1: exit $rc out '$out' write '$w'; want $5 '$6' '$7'"; fails=1; fi
}
nl='
'
case_ 'first calendar release after 4.6' 'version = "4.6.61"' 'refs/tags/v4.6.91 old' 0 0 tag=v4.261107.1 v4.261107.1
case_ 'no tags: UTC floor' 'version = "4.6.61"' '' 0 0 tag=v4.261107.1 v4.261107.1
case_ 'same day numeric 9 to 10' 'version = "4.6.61"' "refs/tags/v4.261107.8 d8${nl}refs/tags/v4.261107.9 d9" 0 0 tag=v4.261107.10 v4.261107.10
UTC_DAY=261108 case_ 'new UTC day resets N' 'version = "4.6.61"' 'refs/tags/v4.261107.99 old' 0 0 tag=v4.261108.1 v4.261108.1
UTC_DAY=270101 case_ 'new UTC year resets N' 'version = "4.6.61"' 'refs/tags/v4.261231.99 old' 0 0 tag=v4.270101.1 v4.270101.1
case_ 'clock rollback preserves monotonic maximum' 'version = "4.6.61"' 'refs/tags/v4.261108.9 old' 0 0 tag=v4.261108.10 v4.261108.10
UTC_DAY=000101 case_ 'century rollback cannot downgrade existing tags' 'version = "4.6.61"' 'refs/tags/v4.991231.9 old' 0 0 tag=v4.991231.10 v4.991231.10
case_ 'manifest above UTC and tags wins' 'version = "5.0.0"' 'refs/tags/v4.261107.9 old' 0 0 tag=v5.0.0 v5.0.0
case_ 'other tag names ignored' 'version = "4.6.61"' "refs/tags/v4.261107.3 d3${nl}refs/tags/v9.9.9-rc1 dx${nl}refs/tags/vx d4" 0 0 tag=v4.261107.4 v4.261107.4
UTC_DAY=503 case_ 'unreadable UTC clock refuses writes' 'version = "4.6.61"' '' 0 1 '' ''
UTC_DAY=bad case_ 'malformed UTC date refuses writes' 'version = "4.6.61"' '' 0 1 '' ''
case_ 'calendar commit tagged, draft reuses original date' 'version = "4.6.61"' 'refs/tags/v4.261106.2 abc' draft 0 tag=v4.261106.2 ''
case_ 'calendar commit published never retags' 'version = "4.6.61"' 'refs/tags/v4.261106.2 abc' 1 0 '' ''
case_ 'commit tagged, unreleased: reuses tag' 'version = "1.1.0"' 'refs/tags/v1.1.3 abc' 0 0 tag=v1.1.3 ''
case_ 'commit tagged, draft release: reuses tag' 'version = "1.1.0"' 'refs/tags/v1.1.3 abc' draft 0 tag=v1.1.3 ''
case_ 'commit tagged, released: nothing' 'version = "1.1.0"' 'refs/tags/v1.1.3 abc' 1 0 '' ''
case_ 'tag list 503: fails' 'version = "1.1.0"' 503 0 1 '' ''
case_ 'invalid version: fails' 'version = "x"' '' 0 1 '' ''
GITHUB_REF_TYPE=tag GITHUB_REF_NAME=v9.9.9 case_ 'manual tag push: releases it' 'version = "1.1.0"' 503 0 0 tag=v9.9.9 ''
# Main demands can arrive after a newer release; they must never roll it back.
GITHUB_REF_TYPE=branch
GITHUB_REF_NAME=main LATEST=v1.1.3 COVERAGE=behind case_ 'main demand: new source releases' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 0 tag=v4.261107.1 v4.261107.1
GITHUB_REF_NAME=main LATEST=v1.1.3 COVERAGE=identical case_ 'main demand: same source skips' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 0 '' ''
GITHUB_REF_NAME=main LATEST=v1.1.3 COVERAGE=ahead case_ 'main demand: already included source skips' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 0 '' ''
GITHUB_REF_NAME=main LATEST=v1.1.3 COVERAGE=diverged case_ 'main demand: divergent source refuses' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 1 '' ''
GITHUB_REF_NAME=main LATEST=v1.1.3 COVERAGE=503 case_ 'main demand: unknown ancestry refuses' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 1 '' ''
GITHUB_REF_NAME=main LATEST=503 COVERAGE=behind case_ 'main demand: unavailable latest refuses' 'version = "1.1.0"' 'refs/tags/v1.1.3 old' 1 1 '' ''
# Actual index helper carries precisely the selected calendar version for all OS names.
python3 - "$src" <<'PYINDEX'
import importlib.util, pathlib, sys
p=pathlib.Path(sys.argv[1]).with_name('release-index.py')
spec=importlib.util.spec_from_file_location('release_index',p);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
v='4.261107.10';files={f'taskq-{v}-{os}':'a'*64 for os in ['linux-x86_64','windows-x86_64.exe','macos-aarch64']}
sums,index=m.index(v,'',files)
assert index['version']==v
assert set(index['files'])=={'linux-x86_64','windows-x86_64','macos-aarch64'}
assert all(name.startswith('taskq-'+v+'-') for name in index['files'].values())
assert len(sums.splitlines())==3
print('ok   calendar identity preserved by actual all-OS index helper')
PYINDEX
exit $fails
