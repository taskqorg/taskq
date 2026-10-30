"""#1957 A4: macOS cached reads with a release binary in an isolated installation; no board token, no live report.
Usage: python3 measure.py TASKQ_BINARY. Adapted from docs/design/evidence/1940/measure.py on PR #1956."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time

binary = Path(sys.argv[1]).resolve()
root = Path(tempfile.mkdtemp(prefix="taskq-1957-read-"))
(root / ".taskq").mkdir()
(root / "taskq.json").write_text('{"board":"github","repo":"invented/reports"}')
status = root / ".taskq/status.json"
sha = "a" * 40
body = "# Question\n\nHow should interrupted requests recover?\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```"
reports = [dict(n=n, title=f"Invented report {n}", sha=sha, closed=100-n, read=False, visible=True, body=body, tasks=[]) for n in range(1, 10)]
ordinary = [dict(n=n, title=f"Invented completed task {n}", kind="task", sha=sha, closed=1100-n, read=False, visible=True,
                 body="Changed: Recovered once.\nChecks: replay -- passed\nOpen: none") for n in range(101, 111)]
env = {k: v for k, v in os.environ.items() if k not in ("TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "GH_TOKEN", "GITHUB_TOKEN", "CODEX_THREAD_ID", "COREQ_TRACE_CONTEXT")}
env.update(TASKQ_ROOT=str(root), TASKQ_INSTALL=str(root / "install"), TASKQ_INCIDENT_REPORTS="off")


def run(args):
    start = time.perf_counter()
    result = subprocess.run([str(binary), *args], cwd=root, env=env, capture_output=True, timeout=60)
    assert result.returncode == 0, result.stderr.decode(errors="replace")[-1000:]
    return result.stdout.decode("utf-8"), round((time.perf_counter() - start) * 1000, 3)


def write(value):
    text = json.dumps(value)
    status.write_text(text)
    return text


commands = []
for data, calls in ((reports, (["reports", "--json"], ["reports", "1", "--sha", sha, "--action", "open", "--json"])),
                    (ordinary+reports, (["results", "--json"], ["results", "101", "--sha", sha, "--json"], ["results", "102", "--sha", sha, "--json"]))):
    snapshot = write(dict(reports=dict(complete=True, observed=int(time.time()), reports=data)))
    for args in calls:
        out, ms = run(args)
        assert json.loads(out)["complete"] is True
        commands.append(dict(command="taskq " + " ".join(args), collected=len(data), wall_ms=ms, outcome="ok"))
    assert status.read_text() == snapshot, "a cached read rewrote the collection"

# Unavailable and stale collections must stay unknown with an explicit reason, never an empty success.
unknown = []
cases = (("missing status.json", None),
         ("stale collection", dict(reports=dict(complete=True, observed=int(time.time())-86400, reports=reports))),
         ("service error", dict(error="Board unavailable; collection unknown", reports=dict(complete=True, observed=int(time.time()), reports=reports))),
         ("invalid collection", dict(reports=dict(observed=int(time.time()), reports="broken"))))
for name, value in cases:
    if value is None:
        status.unlink(missing_ok=True)
    else:
        write(value)
    for args in (["reports", "--json"], ["results", "--json"]):
        out, ms = run(args)
        got = json.loads(out)
        assert got["complete"] is False and got.get("reason"), (name, got)
        unknown.append(dict(case=name, command="taskq " + " ".join(args), complete=False, reason=got["reason"], wall_ms=ms))

events = [json.loads(line).get("observation", {}) for line in run(["profile", "export", "--since", "1h"])[0].splitlines()]
timings = [dict(operation=e["operation"], duration_ms=round(e["duration_us"] / 1000, 3), outcome=e.get("outcome"))
           for e in events if ".cached" in e.get("operation", "") and e.get("duration_us") is not None]
operations = sorted({e["operation"] for e in events if e.get("operation")})
# Collection or any board/provider request during these reads would be a board read; telemetry and identity are local.
board = [o for o in operations if any(w in o for w in ("collect", "board", "github", "gitlab", "issue", "http"))]
assert all(any(t["operation"].endswith(op) for t in timings) for op in ("reports.list.cached", "reports.open.cached")), "load timings missing"
assert not board, f"board operations recorded: {board}"
result = dict(qualification=f"macOS {platform.mac_ver()[0]} {platform.machine()} isolated CLI; release binary; 9 invented reports and 10 completed tasks; no board token; not deployed",
              binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
              commands=commands, unknown=unknown, cached_operations=timings, operations=operations, board_operations=board, source_unchanged=True)
Path(__file__).with_name("cached-reads.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps(result, indent=2))
