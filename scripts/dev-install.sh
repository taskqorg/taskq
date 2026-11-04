#!/bin/sh
# taskq.md § 8 item 1: install a local development build of origin/main (or the ref given) on this Mac.
# Version <next patch of Cargo.toml or the highest release tag>-yymmdd-N (UTC date, N = that date's builds on this host from 0; scripts/dev-version.py) sorts above the release on main and below the next one, so `current`
# moves forward (coreq `newer`). The host channel is set to `dev` first, so no release replaces the build until
# `taskq service channel live` (#1325). Builds in a detached worktree beside the
# primary checkout, so local edits there are never touched. Under a running host service, install makes the build
# current and waits until the service re-execs into it, handing running children over (#894, #1306).
# On channel dev the host service runs this on request (`taskq service dev-build`) for the newest main SHA with green CI (docs/design/host-service.md#dev-install):
# `dev/lock` keeps one install at a time, `dev/result` reports the outcome to the service.
set -eu
d=$HOME/.local/share/taskq
repo=${TASKQ_DEV_REPO:-$(cd "$(dirname "$0")/.." && cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)}
w=${TASKQ_DEV_WORKTREE:-$repo/.worktrees/dev-build}
ref=${1:-origin/main}
mkdir -p "$d/dev"
if ! mkdir "$d/dev/lock" 2>/dev/null; then
  pid=$(cat "$d/dev/lock/pid" 2>/dev/null || true)
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then echo "Error: dev install $pid runs; not started" >&2; exit 1; fi
  rm -rf "$d/dev/lock"; mkdir "$d/dev/lock"
fi
echo $$ > "$d/dev/lock/pid"
sha=$ref
finish() {
  rc=$?
  [ -d "$w" ] && git -C "$w" checkout -q -- Cargo.toml Cargo.lock 2>/dev/null || true
  if [ "$rc" = 0 ]; then echo "$sha installed"; else echo "$sha failed"; fi > "$d/dev/result"
  rm -rf "$d/dev/lock"
}
trap finish EXIT
trap 'exit 1' INT TERM HUP
# A given ref (the service's SHA) needs no origin/main update, so a concurrent fetch holding its lock cannot fail it (#1633).
if [ $# -gt 0 ]; then git -C "$repo" fetch -q --tags --refmap= --no-write-fetch-head origin main; else git -C "$repo" fetch -q --tags origin main; fi
[ -d "$w" ] || git -C "$repo" worktree add -q --detach "$w" "$ref"
cd "$w"
git checkout -q -f --detach "$ref"
sha=$(git rev-parse HEAD)
# shellcheck disable=SC2046 # one argument per release tag
v=$(python3 scripts/dev-version.py Cargo.toml "$d/releases" $(git tag -l 'v[0-9]*'))
prev=$(cat "$d/current")
sed -i '' "s/^version = .*/version = \"$v\"/" Cargo.toml
npm ci --silent
# § 8 step 5: cargo run goes through the macOS signing runner, so the build is signed before its first start.
# A host tool gap must not hold the build that may fix it (docs/design/host-service.md#host-tools).
TASKQ_SELFTEST_TOOLS=off cargo run -q --release -- selftest
p=$(head -1 "$d/projects")
(cd "$p" && "$w/target/release/taskq" service channel dev && "$w/target/release/taskq" service install)
now=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$d/service" 2>/dev/null || echo unknown)
[ "$now" = "$v" ] || { echo "Error: host service runs $now, not $v; not installed" >&2; exit 1; }
(cd "$p" && "$d/taskq" service channel)
echo "installed $v from $(git rev-parse --short HEAD); was $prev"
echo "rollback: echo $prev > $d/current, then run '$d/taskq service restart' once (it restarts the host service for every project)"
