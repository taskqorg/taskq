#!/bin/sh
# taskq.md § 8 item 1: install a local development build of origin/main (or the ref given) on this Mac.
# Version <next patch>-dev.<unix seconds> sorts above the release on main and below the next one, so `current`
# moves forward (coreq `newer`). The host channel is set to `dev` first, so no release replaces the build until
# `taskq service channel live` (#1325). Builds in a detached worktree beside the
# primary checkout, so local edits there are never touched. Under a running host service, install makes the build
# current and waits until the service re-execs into it, handing running children over (#894, #1306).
# On channel dev the host service runs this for each new main SHA with green CI (docs/design/host-service.md#dev-install):
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
git -C "$repo" fetch -q origin main
[ -d "$w" ] || git -C "$repo" worktree add -q --detach "$w" "$ref"
cd "$w"
git checkout -q -f --detach "$ref"
sha=$(git rev-parse HEAD)
v=$(python3 -c 'import re,time; m=re.search(r"^version = \"(\d+)\.(\d+)\.(\d+)\"$", open("Cargo.toml").read(), re.M); print(f"{m[1]}.{m[2]}.{int(m[3])+1}-dev.{int(time.time())}")')
prev=$(cat "$d/current")
sed -i '' "s/^version = .*/version = \"$v\"/" Cargo.toml
npm ci --silent
# § 8 step 5: cargo run goes through the macOS signing runner, so the build is signed before its first start
cargo run -q --release -- selftest
p=$(head -1 "$d/projects")
(cd "$p" && "$w/target/release/taskq" service channel dev && "$w/target/release/taskq" service install)
now=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$d/service" 2>/dev/null || echo unknown)
[ "$now" = "$v" ] || { echo "Error: host service runs $now, not $v; not installed" >&2; exit 1; }
(cd "$p" && "$d/taskq" service channel)
echo "installed $v from $(git rev-parse --short HEAD); was $prev"
echo "rollback: echo $prev > $d/current, then run '$d/taskq service restart' once (it restarts the host service for every project)"
