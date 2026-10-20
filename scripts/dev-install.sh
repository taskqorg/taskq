#!/bin/sh
# taskq.md § 8 item 1: install a local development build of origin/main (or the ref given) on this Mac.
# Version <next patch>-dev.<unix seconds> sorts above the release on main and below the next one, so `current`
# moves forward (coreq `newer`). The host channel is set to `dev` first, so no release replaces the build until
# `taskq service channel live` (#1325). Builds in a detached worktree beside the
# primary checkout, so local edits there are never touched. Under a running host service, install makes the build
# current and waits until the service re-execs into it, handing running children over (#894, #1306).
set -eu
d=$HOME/.local/share/taskq
repo=$(cd "$(dirname "$0")/.." && cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)
w=${TASKQ_DEV_WORKTREE:-$repo/.worktrees/dev-build}
ref=${1:-origin/main}
git -C "$repo" fetch -q origin main
[ -d "$w" ] || git -C "$repo" worktree add -q --detach "$w" "$ref"
cd "$w"
git checkout -q -f --detach "$ref"
v=$(python3 -c 'import re,time; m=re.search(r"^version = \"(\d+)\.(\d+)\.(\d+)\"$", open("Cargo.toml").read(), re.M); print(f"{m[1]}.{m[2]}.{int(m[3])+1}-dev.{int(time.time())}")')
prev=$(cat "$d/current")
trap 'git checkout -q -- Cargo.toml Cargo.lock' EXIT
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
