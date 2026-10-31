#!/bin/sh
# Chain test for docs/shots/update.sh (#2024 A1-A3, A7) with stub uname, cargo, npm, gh, node and taskq binaries:
# a published set records REF, binary origin and reuse key; a matching key prints `fresh` without capture; a failed
# build, capture, hash, attestation, version or preview check and a missing image keep docs/shots unchanged and print
# exactly one `update.sh: <stage>:` line; capture sees no board token or project root and gets no service command.
set -eu
here=$(cd "$(dirname "$0")/../.." && pwd)
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/repo"
stub() { cat > "$t/bin/$1"; chmod +x "$t/bin/$1"; }
stub uname <<'EOF'
#!/bin/sh
case ${1:-} in -m) echo arm64 ;; *) echo "${STUB_OS:-Darwin}" ;; esac
EOF
stub sw_vers <<'EOF'
#!/bin/sh
echo 27.0
EOF
stub osascript <<'EOF'
#!/bin/sh
echo "${STUB_SCALE:-2}"
EOF
stub npm <<'EOF'
#!/bin/sh
mkdir -p node_modules
EOF
stub cargo <<'EOF'
#!/bin/sh
[ -z "${STUB_BUILD_FAIL:-}" ] || { echo 'error: could not compile' >&2; exit 101; }
mkdir -p "$CARGO_TARGET_DIR/release" && cp "$STUB_DIR/taskq" "$CARGO_TARGET_DIR/release/taskq"
EOF
stub gh <<'EOF'
#!/bin/sh
echo "gh $*" >> "$STUB_DIR/calls"
case "$1 $2" in
  'release download') while [ "$1" != --dir ]; do shift; done; printf '%s\n' "$STUB_SUMS" > "$2/SHA256SUMS" ;;
  'attestation verify') exit "${STUB_ATTEST:-0}" ;;
esac
EOF
stub node <<'EOF'
#!/bin/sh
[ -z "${STUB_PREVIEW_FAIL:-}" ] || { echo '390 px: horizontal overflow' >&2; exit 1; }
mkdir -p "$5" && echo '{"checks": []}' > "$5/page-checks.json"
EOF
# The taskq binary: version, signing and the two snapshot routes; records each capture's arguments and environment.
cat > "$t/taskq" <<'EOF'
#!/bin/sh
case $1 in
  --version) echo "${STUB_VERSION:-1.2.3}" ;;
  sign-local) ;;
  tray)
    echo "$*" >> "$STUB_DIR/calls"; env >> "$STUB_DIR/capture-env"
    [ -z "${STUB_CAPTURE_FAIL:-}" ] || { echo 'snapshot: renderer failed' >&2; exit 1; }
    mkdir -p "$3"; png() { printf '\211PNG\r\n\032\n%s %s' "$1" "${STUB_SCALE:-2}" > "$2"; }
    case $5 in
      project,icon) png project "$3/popover-dark-project-16.png"; png icon "$3/icon-summary-run-dark-16.png" ;;
      conversations) png owner "$3/popover-dark-conversations-owner-18.png"
        [ -n "${STUB_NO_QUESTION:-}" ] || png question "$3/popover-dark-conversations-question-18.png" ;;
    esac ;;
  *) echo "unexpected taskq $*" >> "$STUB_DIR/calls"; exit 1 ;;
esac
EOF
chmod +x "$t/taskq"
export PATH="$t/bin:$PATH" STUB_DIR="$t" GH_TOKEN=board-token TASKQ_ROOT=/project
cd "$t/repo"
git init -q . && git config user.email t@t && git config user.name t && git config core.autocrlf false
mkdir -p src scripts docs/shots tests/fixtures
cp "$here/docs/shots/update.sh" "$here/docs/shots/check.sh" "$here/docs/shots/fixture.sh" docs/shots/
ui=$(python3 -c "import re,sys; print(*re.search(r\"UI = '''(.*?)'''\", open(sys.argv[1]).read(), re.S).group(1).split())" docs/shots/check.sh | tr -d '\r')
for f in $ui tests/fixtures/conversation-shots.js Cargo.lock package-lock.json scripts/readme-preview.cjs; do echo "$f" > "$f"; done
printf '[package]\nname = "taskq"\nversion = "1.2.3"\n' > Cargo.toml
printf '![](docs/shots/popover-dark-%s.png)\n' project-16 conversations-owner-18 conversations-question-18 > README.md
printf 'target/\n' > .gitignore
git add -A && git commit -qm v1.2.3 && git tag v1.2.3
fails=0
ok() { echo "ok   $1"; }
no() { echo "FAIL $1"; fails=1; }
run() { # run update.sh, keep stdout in out and stderr in err; returns its exit code
  rc=0; docs/shots/update.sh "$@" > "$t/out" 2> "$t/err" || rc=$?; return 0
}
same() { git diff --quiet HEAD -- docs/shots && [ -z "$(git status --porcelain docs/shots)" ]; }
args=''
failed() { # $1: name, $2: stage; rest: environment for the run; arguments in $args
  name=$1 stage=$2; shift 2
  rc=0; env "$@" docs/shots/update.sh $args > "$t/out" 2> "$t/err" || rc=$?
  lines=$(grep -c . "$t/err" || true)
  if [ "$rc" = 1 ] && same && [ "$lines" = 1 ] && grep -q "^update.sh: $stage: .*; previous set kept (log: " "$t/err"; then ok "$name"
  else no "$name: exit $rc, $lines stderr lines, docs/shots $(same && echo kept || echo changed): $(cat "$t/err")"; fi
}

run v1.2.3 --publish
m=docs/shots/manifest.json
if [ "$rc" = 0 ] && grep -q '"ref": "v1.2.3"' $m && grep -q '"binaryOrigin": "build"' $m && grep -q '"scale": "2"' $m &&
   [ -f docs/shots/popover-dark-conversations-question-18.png ] && [ -f docs/shots/icon-summary-run-dark.png ] &&
   [ -f docs/shots/evidence/1.2.3/page-checks.json ] && docs/shots/check.sh 2>/dev/null; then ok 'publish writes images, manifest and preview'
else no "publish: exit $rc $(cat "$t/err")"; fi
if grep -q 'GH_TOKEN\|TASKQ_ROOT' "$t/capture-env" || grep -qi service "$t/calls" || grep -q '^gh ' "$t/calls"; then
  no 'capture environment: board token, project root, service command or gh call seen'
else ok 'capture has no board token, project root, service command or gh call'; fi
git add -A && git commit -qm shots
: > "$t/calls"
run v1.2.3 --publish
if [ "$rc" = 0 ] && grep -q '^fresh [0-9a-f]\{64\}$' "$t/out" && [ ! -s "$t/calls" ] && same; then ok 'matching reuse key is fresh without capture'
else no "fresh: exit $rc, calls $(cat "$t/calls") $(cat "$t/out" "$t/err")"; fi
# A new display scale changes the key, so each failure below would otherwise capture and publish.
args='v1.2.3 --publish'
failed 'build failure' build STUB_SCALE=1 STUB_BUILD_FAIL=1
failed 'capture failure' capture STUB_SCALE=1 STUB_CAPTURE_FAIL=1
failed 'missing image' image STUB_SCALE=1 STUB_NO_QUESTION=1
failed 'binary version mismatch' binary STUB_SCALE=1 STUB_VERSION=1.2.4
failed 'preview failure' preview STUB_SCALE=1 STUB_PREVIEW_FAIL=1
failed 'unsupported platform' platform STUB_OS=Linux
args=v9.9.9
failed 'unknown ref' ref STUB_SCALE=1
cp "$t/taskq" "$t/release-taskq" && echo '# release build' >> "$t/release-taskq"
sum=$(sha256sum "$t/release-taskq" | cut -d' ' -f1)
args="v1.2.3 --bin $t/release-taskq --publish"
failed 'release hash mismatch' hash STUB_SUMS="$(printf '%064d  taskq-1.2.3-macos-aarch64' 0)"
failed 'attestation failure' attestation STUB_SUMS="$sum  taskq-1.2.3-macos-aarch64" STUB_ATTEST=1
rc=0; STUB_SUMS="$sum  taskq-1.2.3-macos-aarch64" docs/shots/update.sh v1.2.3 --bin "$t/release-taskq" --publish > "$t/out" 2> "$t/err" || rc=$?
if [ "$rc" = 0 ] && grep -q '"binaryOrigin": "release v1.2.3"' $m && grep -q "\"binarySha256\": \"$sum\"" $m && docs/shots/check.sh 2>/dev/null
then ok 'verified release binary publishes'; else no "release binary: exit $rc $(cat "$t/err")"; fi
exit $fails
