#!/bin/sh
# #1699 A8: native Conversations captures (WKWebView snapshot of the workspace bundle at 800x600) over
# docs/shots/fixture.sh plus the invented state in tests/fixtures/conversation-shots.js. Usage: capture.sh DIR, from the repository root.
set -eu
d=${1:?usage: capture.sh DIR}
docs/shots/fixture.sh "$d" >/dev/null
d=$(cd "$d" && pwd -P)
TASKQ_INSTALL=$d/install cargo run --locked -- tray --snapshot "$d/shots" --route conversations
