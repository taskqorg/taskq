#!/bin/sh
# #1395 A1: read-only evidence for the Screen Recording prompt. Run by the owner in Terminal on the Mac.
# Prints the service process path, whether <install>/taskq meets the stable requirement, recent tccd
# ScreenCapture decisions and the TCC.db rows; a failed requirement check of <install>/taskq is the bundle trap,
# a grant that pins an older cdhash is a stale grant (#1803; docs/design/worker-permissions.md#macos-signing-identity).
# Never edits TCC.db, never runs tccutil, never triggers a prompt (A3).
# TCC.db needs Full Disk Access for Terminal; without it that part prints unknown.
# `--label ok|failed` labels tccd lines and TCC.db rows on stdin; the word is the `codesign -v` outcome of <install>/taskq.

# A cdhash requirement is opCDHash (8) with a 20-byte hash: hex 00000008 00000014 in the csreq blob.
label() {
  awk -v cs="$1" '
    /code has no resources but signature indicates they must be present/ { print $0 "  <- bundle trap"; next }
    /Failed to match existing code requirement for subject .*\/taskq/ { print $0 (cs == "ok" ? "  <- stale grant" : "  <- bundle trap"); next }
    /\|[0-9A-F]*0000000800000014[0-9A-F]*$/ { print $0 "  <- stale grant (cdhash-pinned row)"; next }
    { print }'
}
[ "$1" = --label ] && { label "$2"; exit; }

I="${TASKQ_INSTALL:-$HOME/.local/share/taskq}"
echo "== current release: $(cat "$I/current" 2>/dev/null || echo unknown)"
pid=$(sed -n 's/.*"pid":\([0-9]*\).*/\1/p' "$I/service" 2>/dev/null)
echo "== service pid ${pid:-unknown}"
[ -n "$pid" ] && ps -o pid,ppid,comm -p "$pid"
[ -n "$pid" ] && exe=$(ps -o comm= -p "$pid") && codesign -d -r- "$exe" 2>&1 | grep designated
echo "== <install>/taskq against the stable requirement"
cs=failed
codesign -v --strict "$I/taskq" 2>&1 && cs=ok
echo "codesign -v: $cs (failed is the bundle trap)"
leaf=$(codesign -d -r- "$I/taskq" 2>&1 | sed -n 's/.*certificate leaf = H"\([0-9a-f]*\)".*/\1/p')
codesign -v --strict -R="identifier \"org.taskq.taskq\" and certificate leaf = H\"$leaf\"" "$I/taskq" 2>&1 && echo "ok: org.taskq.taskq, leaf ${leaf:-none}"
echo "== service children (workers and their tools)"
[ -n "$pid" ] && pgrep -lP "$pid"
echo "== Claude daemon processes (claude --bg sessions are hosted here)"
pgrep -lf 'claude.*(daemon|agents|--bg)' | cut -c1-160
echo "== tccd ScreenCapture decisions, last 48 h (client path or bundle id, responsible process)"
/usr/bin/log show --last 48h --style compact --predicate \
  'subsystem == "com.apple.TCC" AND (eventMessage CONTAINS[c] "ScreenCapture" OR eventMessage CONTAINS "Failed to match" OR eventMessage CONTAINS "has no resources")' 2>&1 \
  | grep -i -E 'taskq|responsible|AUTHREQ' | tail -40 | cut -c1-400 | label "$cs"
echo "== TCC.db ScreenCapture rows (client, client_type 0=bundle id 1=path, auth_value 2=allowed, modified, csreq hex)"
rows=$(sqlite3 "$HOME/Library/Application Support/com.apple.TCC/TCC.db" \
  "select client, client_type, auth_value, datetime(last_modified,'unixepoch'), hex(csreq) from access where service='kTCCServiceScreenCapture';" 2>&1) \
  && echo "$rows" | label "$cs" || echo "unknown: Terminal lacks Full Disk Access"
