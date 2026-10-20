#!/bin/sh
# #1395 A1: read-only evidence for the Screen Recording prompt. Run by the owner in Terminal on the Mac.
# Prints the service process path, whether <install>/taskq meets the stable requirement, recent tccd
# ScreenCapture decisions and the TCC.db rows; a failed requirement check of <install>/taskq is the bundle trap
# (docs/design/worker-permissions.md#macos-signing-identity). Never edits TCC.db, never runs tccutil, never triggers a prompt (A3).
# TCC.db needs Full Disk Access for Terminal; without it that part prints unknown.
I="${TASKQ_INSTALL:-$HOME/.local/share/taskq}"
echo "== current release: $(cat "$I/current" 2>/dev/null || echo unknown)"
pid=$(sed -n 's/.*"pid":\([0-9]*\).*/\1/p' "$I/service" 2>/dev/null)
echo "== service pid ${pid:-unknown}"
[ -n "$pid" ] && ps -o pid,ppid,comm -p "$pid"
[ -n "$pid" ] && exe=$(ps -o comm= -p "$pid") && codesign -d -r- "$exe" 2>&1 | grep designated
echo "== <install>/taskq against the stable requirement"
leaf=$(codesign -d -r- "$I/taskq" 2>&1 | sed -n 's/.*certificate leaf = H"\([0-9a-f]*\)".*/\1/p')
codesign -v --strict -R="identifier \"org.taskq.taskq\" and certificate leaf = H\"$leaf\"" "$I/taskq" 2>&1 && echo "ok: org.taskq.taskq, leaf ${leaf:-none}"
echo "== service children (workers and their tools)"
[ -n "$pid" ] && pgrep -lP "$pid"
echo "== Claude daemon processes (claude --bg sessions are hosted here)"
pgrep -lf 'claude.*(daemon|agents|--bg)' | cut -c1-160
echo "== tccd ScreenCapture decisions, last 48 h (client path or bundle id, responsible process)"
/usr/bin/log show --last 48h --style compact --predicate \
  'subsystem == "com.apple.TCC" AND eventMessage CONTAINS[c] "ScreenCapture"' 2>&1 \
  | grep -i -E 'taskq|responsible|AUTHREQ' | tail -40 | cut -c1-400
echo "== TCC.db ScreenCapture rows (client, client_type 0=bundle id 1=path, auth_value 2=allowed)"
sqlite3 "$HOME/Library/Application Support/com.apple.TCC/TCC.db" \
  "select client, client_type, auth_value, datetime(last_modified,'unixepoch') from access where service='kTCCServiceScreenCapture';" 2>&1 \
  || echo "unknown: Terminal lacks Full Disk Access"
