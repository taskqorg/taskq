#!/bin/sh
# One-command README screenshot refresh (docs/design/readme-plan.md, Images and acceptance; #1919, #2024):
#   docs/shots/update.sh [REF] [--bin PATH] [--publish]
# Captures the README request, queue and question images and the summary icon of REF (default HEAD) with the real
# `taskq tray --snapshot` WKWebView renderer, from one --locked build in a detached worktree of REF or from the verified
# release binary PATH (SHA256SUMS, build provenance attestation, --version). Images, manifest.json and a README preview
# checked at 1280 and 390 px are staged first; docs/shots/ changes only with --publish and only after every check
# passed. A manifest with the same reuse key prints `fresh <key>` without capture. Any failure prints one
# `update.sh: <stage>: <problem>; previous set kept` line. Never commits, pushes, starts a service or an agent, or sets
# a board token. macOS only; Windows and Linux capture are unsupported.
set -eu
cd "$(dirname "$0")/../.."
root=$(pwd -P)
ref=HEAD bin='' publish=''
while [ $# -gt 0 ]; do
  case $1 in
    --bin) bin=${2:?update.sh: --bin needs a path}; shift 2 ;;
    --publish) publish=1; shift ;;
    -*) echo 'usage: docs/shots/update.sh [REF] [--bin PATH] [--publish]' >&2; exit 2 ;;
    *) ref=$1; shift ;;
  esac
done
tmp=$(mktemp -d "${TMPDIR:-/tmp}/taskq-shots.XXXXXX")
stage=$tmp/stage log=$tmp/update.log
mkdir "$stage"
fail() { echo "update.sh: $1: $2; previous set kept (log: $log)" >&2; exit 1; }
quiet() { "$@" >>"$log" 2>&1; }
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
[ "$(uname -s)" = Darwin ] || fail platform 'capture needs the macOS WKWebView renderer; Windows and Linux are unsupported, dispatch the README shots workflow instead'
commit=$(git rev-parse --verify -q "$ref^{commit}") || fail ref "$ref is not a commit here; fetch it (git fetch --tags) or pass a commit, branch or tag"

# A persistent detached worktree and target directory keep warm builds incremental across runs.
work=${TASKQ_SHOTS_WORK:-$root/target/readme-shots}
src=$work/src
mkdir -p "$work"
if [ -e "$src/.git" ]; then
  quiet git -C "$src" checkout -q -f --detach "$commit" || fail worktree "cannot check out $commit in $src; remove that directory and run git worktree prune"
else
  quiet git worktree prune
  quiet git worktree add -f --detach "$src" "$commit" || fail worktree "cannot create a worktree of $commit in $src; run git worktree prune"
fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$src/Cargo.toml" | head -n1)

exe=$tmp/taskq
if [ -n "$bin" ]; then
  [ -f "$bin" ] || fail binary "$bin does not exist"
  cp "$bin" "$exe" && chmod +x "$exe"
  binsha=$(sha "$exe")
  quiet gh release download "v$version" --repo taskqorg/taskq --pattern SHA256SUMS --dir "$tmp" || fail hash "cannot download SHA256SUMS of release v$version; check gh access and that the release exists"
  grep -Eq "^$binsha [ *]?taskq-$version-" "$tmp/SHA256SUMS" || fail hash "$bin is not a file of release v$version (SHA-256 $binsha not in its SHA256SUMS)"
  quiet gh attestation verify "$exe" --repo taskqorg/taskq || fail attestation "build provenance of $bin does not verify for taskqorg/taskq"
  origin="release v$version"
else
  [ -f "$src/node_modules/.taskq-lock" ] && [ "$(cat "$src/node_modules/.taskq-lock")" = "$(sha "$src/package-lock.json")" ] ||
    { (cd "$src" && quiet npm ci --ignore-scripts --no-audit --no-fund) || fail build "npm ci failed in $src; check Node.js and network"
      sha "$src/package-lock.json" > "$src/node_modules/.taskq-lock"; }
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$work/target}"
  (cd "$src" && quiet cargo build --release --locked) || fail build "cargo build --release --locked failed at $commit; see the log"
  built=$CARGO_TARGET_DIR/release/taskq
  binsha=$(sha "$built")
  # Sign a copy: the build output and its hash stay stable for the reuse key.
  cp "$built" "$exe" && quiet "$exe" sign-local "$exe" || fail build "signing the build copy failed"
  origin=build
fi
[ "$("$exe" --version 2>>"$log")" = "$version" ] || fail binary "binary version is not $version, the Cargo.toml version at $commit"

scale=$(osascript -l JavaScript -e 'ObjC.import("AppKit"); $.NSScreen.mainScreen.backingScaleFactor' 2>/dev/null || echo unknown)
case $scale in [0-9]*) ;; *) scale=unknown ;; esac
# ui: sources whose blob IDs pin the images; inputs: capture inputs hashed by content. check.sh keeps the same lists.
ui='src/tray.rs src/tray.html src/tray-app.js src/tray-entry.js src/task-workspace.js src/task-workspace.css src/tray-view-state.js src/tray-web.js src/app-shell.js src/app-shell.css src/task-dashboard.js src/task-dashboard.css src/task-views.js src/anchored-popover.js src/ui-icons.js src/ui-icons.json src/project-blocks.js src/project-blocks.css src/conversations.rs src/conversations.js src/reports.js src/conversations.css src/pm-composer.js src/ui-runtime.js src/workspace.js src/workspace-entry.js src/workspace-answers.js src/answer-card.js scripts/build-web.cjs src/icon.rs'
inputs='docs/shots/fixture.sh tests/fixtures/conversation-shots.js docs/shots/update.sh Cargo.lock'
# head: everything but images; the reuse key is its SHA-256 with sourceCommit and ref left out.
python3 - "$src" "$commit" "$ref" "$version" "$binsha" "$origin" "macOS $(sw_vers -productVersion)" "$(uname -m)" "$scale" "$ui" "$inputs" > "$tmp/head.json" <<'PY' || fail manifest 'cannot describe the sources; see the log'
import hashlib, json, subprocess, sys
src, commit, ref, version, binsha, origin, osname, arch, scale, ui, inputs = sys.argv[1:]
git = lambda *a: subprocess.run(['git', '-C', src, *a], capture_output=True, check=True).stdout
key = {'version': version, 'binarySha256': binsha,
       'environment': {'os': osname, 'arch': arch, 'renderer': 'WKWebView snapshot', 'scale': scale},
       'uiSources': {f: git('rev-parse', f'{commit}:{f}').decode().strip() for f in ui.split()},
       'captureInputs': {f: hashlib.sha256(git('cat-file', 'blob', f'{commit}:{f}')).hexdigest() for f in inputs.split()}}
reuse = hashlib.sha256(json.dumps(key, sort_keys=True).encode()).hexdigest()
json.dump({'version': version, 'sourceCommit': commit, 'ref': ref, 'binaryOrigin': origin, 'reuseKey': reuse, **key,
           'capture': 'Isolated TASKQ_INSTALL; queue and Conversations showcase fixtures; dark images only.'}, sys.stdout)
PY
key=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["reuseKey"])' "$tmp/head.json")
if [ "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("reuseKey"))' docs/shots/manifest.json 2>/dev/null)" = "$key" ]; then
  rc=0; quiet docs/shots/check.sh || rc=$?
  # exit 2 only says HEAD moved past REF; the images still are REF's
  [ "$rc" = 1 ] || { echo "fresh $key"; rm -rf "$tmp"; exit 0; }
fi

start=$(date +%s)
fx=$tmp/fixture
TASKQ_VERSION=$version quiet "$src/docs/shots/fixture.sh" "$fx" || fail capture 'fixture.sh failed; see the log'
# Only the showcase installation: no board token, no project root, no task identity, no incident reports.
capture() { env -u GH_TOKEN -u GITHUB_TOKEN -u GITLAB_TOKEN -u TASKQ_ROOT -u TASKQ_TASK TASKQ_INCIDENT_REPORTS=off \
  TASKQ_INSTALL="$fx/install" "$exe" tray --snapshot "$@"; }
quiet capture "$tmp/shots" --route project,icon || fail capture 'queue and icon snapshot failed; see the log'
quiet capture "$tmp/conversations" --route conversations || fail capture 'Conversations snapshot failed; see the log'
cp "$tmp/shots/popover-dark-project-16.png" "$tmp/conversations/popover-dark-conversations-owner-18.png" \
  "$tmp/conversations/popover-dark-conversations-question-18.png" "$stage/" 2>>"$log" || fail image 'a README image was not rendered; see the log'
# Menu-bar size and scale differ between hosts; keep the README link stable.
cp "$tmp"/shots/icon-summary-run-dark-*.png "$stage/icon-summary-run-dark.png" 2>>"$log" || fail image 'the summary icon was not rendered'
python3 - "$tmp/head.json" "$stage" > "$stage/manifest.json" <<'PY' || fail manifest 'cannot write the manifest'
import hashlib, json, pathlib, sys
m, d = json.load(open(sys.argv[1])), pathlib.Path(sys.argv[2])
m['images'] = [{'file': p.name, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(d.glob('*.png'))]
json.dump(m, sys.stdout, indent=2); print()
PY
# The current check.sh verifies the stage against REF: README links, image hashes and PNGs, sources, version.
cp docs/shots/check.sh "$src/docs/shots/check.sh"
rc=0; (cd "$src" && quiet docs/shots/check.sh "$stage") || rc=$?
quiet git -C "$src" checkout -q -- docs/shots/check.sh
[ "$rc" = 0 ] || fail verify "the staged set fails check.sh (exit $rc); see the log"
echo "update.sh: staged in $(( $(date +%s) - start )) s from capture start: $stage"

node scripts/readme-preview.cjs "$src/README.md" "$src" "$stage" "$tmp/preview" >>"$log" 2>&1 ||
  fail preview "README preview failed at 1280 or 390 px; with no browser run npm i --no-save playwright@1.64.1 && npx playwright install chromium-headless-shell"
if [ -z "$publish" ]; then
  echo "update.sh: preview $tmp/preview; nothing published (pass --publish to replace docs/shots)"
  exit 0
fi
# Staged replacement: images first, the manifest last; check.sh catches an interrupted swap and
# `git checkout -- docs/shots` restores the committed set.
rm -rf docs/shots/.next && cp -R "$stage" docs/shots/.next || fail publish 'cannot copy the stage into docs/shots/.next'
swap() {
  for f in docs/shots/.next/*.png; do mv -f "$f" docs/shots/ || return 1; done
  for f in docs/shots/*.png; do [ -e "$stage/${f##*/}" ] || rm -f "$f" || return 1; done
  mv -f docs/shots/.next/manifest.json docs/shots/manifest.json && rm -rf docs/shots/.next "docs/shots/evidence/$version" &&
    mkdir -p docs/shots/evidence && cp -R "$tmp/preview" "docs/shots/evidence/$version"
}
rc=0; { swap 2>>"$log" && quiet docs/shots/check.sh; } || rc=$?
[ "$rc" != 1 ] || { echo "update.sh: publish: docs/shots is partly replaced; run git checkout -- docs/shots and retry (log: $log)" >&2; exit 1; }
echo "update.sh: docs/shots updated to $version ($origin, $commit) in $(( $(date +%s) - start )) s from capture start; preview docs/shots/evidence/$version"
rm -rf "$tmp"
