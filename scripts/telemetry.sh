#!/bin/sh
# Read-only telemetry proof for the host it runs on (#2282; the mac and pds18 proofs reuse it).
#
#   scripts/telemetry.sh health
#     Telemetry health: installed release, collector health, `profile export` and `profile watch` probes.
#     Verdict FAIL: export fails or the watch Board line is unknown.
#   scripts/telemetry.sh trace N [--since 24h] [--project OWNER/REPO]
#     Every retained worker, stage and transition observation of task N from `profile export` alone, then
#     per attempt which lifecycle events are present (spawn, heartbeat, result, exit, slot-hold, slot-release),
#     and the close transition. PASS needs every attempt complete and the task closed in the window.
#
# Env: TASKQ_INSTALL (default: %LOCALAPPDATA%\taskq or ~/.local/share/taskq), TASKQ (binary; default:
# <install>/taskq[.exe], then taskq on PATH), TASKQ_FIX (commit the release must contain; default #2280).
# Writes nothing outside a temporary file; never runs board commands. Exit: 0 PASS, 1 FAIL, 2 unknown or usage.
set -u

usage() { sed -n '3,13p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }
unix() { command -v cygpath >/dev/null 2>&1 && cygpath -u "$1" || printf '%s\n' "$1"; }

if [ -n "${TASKQ_INSTALL:-}" ]; then I=$(unix "$TASKQ_INSTALL")
elif [ -n "${LOCALAPPDATA:-}" ]; then I="$(unix "$LOCALAPPDATA")/taskq"
else I="$HOME/.local/share/taskq"; fi
if [ -n "${TASKQ:-}" ]; then T=$TASKQ
elif [ -x "$I/taskq.exe" ]; then T="$I/taskq.exe"
elif [ -x "$I/taskq" ]; then T="$I/taskq"
else T=taskq; fi
PY=$(command -v python3 || command -v python) || { echo "unknown: python3 not found"; exit 2; }
FIX=${TASKQ_FIX:-1e5ff561}
TMP=$(mktemp) || exit 2
trap 'rm -f "$TMP"' EXIT

release() {
  v=$("$T" --version 2>/dev/null | tr -d '\r')
  echo "release: ${v:-unknown} (binary $T, install $I)"
  if [ -z "$v" ]; then echo "contains $FIX: unknown (no version)"; return; fi
  dir=$(dirname "$0")
  if ! git -C "$dir" rev-parse -q --verify "v$v^{commit}" >/dev/null 2>&1; then
    git -C "$dir" fetch -q origin "refs/tags/v$v:refs/tags/v$v" >/dev/null 2>&1
  fi
  if git -C "$dir" merge-base --is-ancestor "$FIX" "v$v" 2>/dev/null; then echo "contains $FIX: yes"
  elif git -C "$dir" rev-parse -q --verify "v$v^{commit}" >/dev/null 2>&1 && git -C "$dir" rev-parse -q --verify "$FIX^{commit}" >/dev/null 2>&1; then
    echo "contains $FIX: no"
  else echo "contains $FIX: unknown (tag v$v or $FIX not in this clone)"; fi
}

health() {
  [ $# -eq 0 ] || usage
  fail=""
  release
  "$T" profile health 2>&1 | tr -d '\r' | grep -E '^(CLI|Service) telemetry|^Telemetry store'
  if "$T" profile export --since 30m >"$TMP" 2>&1; then
    echo "export --since 30m: ok, $(($(wc -l <"$TMP") - 1)) rows"
  else
    echo "export --since 30m: FAILED: $(tr -d '\r' <"$TMP" | head -n 1)"; fail="$fail export"
  fi
  board=$("$T" profile watch --since 1h 2>&1 | tr -d '\r' | grep '^Board:')
  echo "watch: ${board:-Board: line missing}"
  case $board in *unknown*|"") fail="$fail watch";; esac
  if [ -n "$fail" ]; then echo "verdict: FAIL ($(echo $fail))"; return 1; fi
  echo "verdict: PASS (export and watch read the store)"
}

trace() {
  [ $# -ge 1 ] || usage
  task=$1; shift; since=24h; project=""
  while [ $# -gt 0 ]; do case $1 in --since) since=$2; shift 2;; --project) project=$2; shift 2;; *) usage;; esac; done
  release
  if ! "$T" profile export --since "$since" >"$TMP" 2>&1; then
    echo "export --since $since: FAILED: $(tr -d '\r' <"$TMP" | head -n 1)"; echo "verdict: unknown (no telemetry)"; return 2
  fi
  "$PY" -I -c '
import json, sys, datetime
task, project, since = int(sys.argv[1]), sys.argv[2], sys.argv[3]
need = ["spawn", "heartbeat", "result", "exit", "slot-hold", "slot-release"]
fields = ["project", "task", "attempt", "host", "runtime", "source", "process_id", "to"]
header = json.loads(sys.stdin.readline())
rows = []
for line in sys.stdin:
    try: o = json.loads(line)["observation"]
    except (ValueError, KeyError, TypeError): continue
    if o.get("task") != task or o.get("kind") not in ("worker", "stage", "transition"): continue
    if project and o.get("project") != project: continue
    rows.append(o)
rows.sort(key=lambda o: o.get("started_ms") or 0)
def utc(ms): return datetime.datetime.fromtimestamp(ms / 1000, datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%S") if ms else "unknown-time"
print("window: since %s; universal %s rows" % (since, header["sources"]["universal"].get("samples")))
if not rows:
    print("task %d: no observation in window" % task); print("verdict: unknown (nothing retained)"); sys.exit(2)
print("UTC | project | kind | role | event | attempt | host | runtime | pid | outcome | reason")
beats = {}
def show(o, note=""):
    kind = o["kind"]
    event = o.get("operation") if kind != "transition" else "%s->%s" % (o.get("from"), o.get("to"))
    reason = o.get("to") if kind == "worker" else (o.get("error") or "")
    print(" | ".join(str(x) for x in (utc(o.get("started_ms")), o.get("project"), kind, o.get("source") if kind == "worker" else "-",
          event, o.get("attempt"), o.get("host"), o.get("runtime"), o.get("process_id") if kind == "worker" else "-",
          o.get("outcome"), reason)) + note)
pending = None
for o in rows:
    hb = o["kind"] == "worker" and o.get("operation") == "heartbeat"
    key = (o.get("project"), o.get("attempt"), o.get("process_id"))
    if hb and pending and pending[0] == key: pending[2] += 1; pending[3] = o; continue
    if pending: show(pending[1], " (x%d heartbeats to %s)" % (pending[2], utc(pending[3].get("started_ms"))) if pending[2] > 1 else "")
    pending = [key, o, 1, o] if hb else None
    if not hb: show(o)
if pending: show(pending[1], " (x%d heartbeats to %s)" % (pending[2], utc(pending[3].get("started_ms"))) if pending[2] > 1 else "")
ok = True
attempts = {}
for o in rows:
    if o["kind"] == "worker": attempts.setdefault((o.get("project"), o.get("attempt")), []).append(o)
for (proj, att), evs in list(attempts.items()):
    # A monitor anomaly (e.g. running-no-process) without any lifecycle event is not an attempt of its own.
    if not {e.get("operation") for e in evs} & set(need):
        print("observation only %s (%s): %s" % (att, proj, ", ".join(sorted({e.get("operation") for e in evs}))))
        del attempts[(proj, att)]
if not attempts: print("no worker events for task %d" % task); ok = False
for (proj, att), evs in attempts.items():
    seen = {e.get("operation") for e in evs}
    missing = [n for n in need if n not in seen]
    def invalid(e, f):
        value = e.get(f)
        if f in ("task", "attempt", "process_id"):
            return type(value) is not int or value <= (1 if f == "process_id" else 0)
        return value in (None, "", "unknown", "other")
    blank = sorted({f for e in evs for f in fields if invalid(e, f)})
    role = "/".join(sorted({str(e.get("source") or "unknown") for e in evs}))
    print("attempt %s (%s, %s): %s%s" % (att, proj, role, "all lifecycle events present" if not missing else "missing " + ", ".join(missing),
          "; invalid identity fields " + ", ".join(blank) if blank else ""))
    ok = ok and not missing and not blank
closed = [o for o in rows if o["kind"] == "transition" and o.get("to") == "closed"]
print("close: %s" % ("%s->closed at %s" % (closed[-1].get("from"), utc(closed[-1].get("started_ms"))) if closed else "none in window"))
if not ok: print("verdict: FAIL (lifecycle gap above)"); sys.exit(1)
if not closed: print("verdict: unknown (every attempt traced; task not closed in window)"); sys.exit(2)
print("verdict: PASS (spawn to close traced for every attempt)")
' "$task" "$project" "$since" <"$TMP"
}

case ${1:-} in
  health) shift; health "$@";;
  trace) shift; trace "$@";;
  *) usage;;
esac
