"""Measure collected reads in an isolated installation; never query or decide a live report."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

project = Path(__file__).resolve().parents[4]
root = Path(tempfile.mkdtemp(prefix="taskq-1940-read-cargo-"))
(root / ".taskq").mkdir()
(root / "taskq.json").write_text('{"board":"github","repo":"invented/reports"}')
sha = "a" * 40
body = "# Question\n\nHow should interrupted requests recover?\n\n# Recommendation\n\nReuse the request identity.\n\n# Proposed tasks\n\n```taskq-tasks\n[]\n```"
rows = [dict(n=n, title=f"Invented report {n}", sha=sha, closed=100-n, read=False,
             visible=True, body=body, tasks=[]) for n in range(1, 10)]
snapshot = json.dumps(dict(reports=dict(complete=True, observed=int(time.time()), reports=rows)))
status = root / ".taskq/status.json"
status.write_text(snapshot)
env = dict(os.environ)
for key in ("TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "GH_TOKEN", "GITHUB_TOKEN", "CODEX_THREAD_ID", "COREQ_TRACE_CONTEXT"):
    env.pop(key, None)
env.update(TASKQ_ROOT=str(root), TASKQ_INSTALL=str(root / "install"), TASKQ_INCIDENT_REPORTS="off")


def run(args):
    result = subprocess.run(["cargo", "+stable", "run", "--locked", "--manifest-path", str(project / "Cargo.toml"), "--", *args],
                            cwd=root, env=env, capture_output=True, timeout=60)
    assert result.returncode == 0, result.stderr.decode(errors="replace")[-1000:]
    return result.stdout.decode("utf-8")


commands = []
ordinary = [dict(n=n, title=f"Invented completed task {n}", kind="task", sha=sha, closed=1100-n,
                 read=False, visible=True, body="Changed: Recovered once.\nChecks: replay -- passed\nOpen: none") for n in range(101,111)]
for data, calls in ((rows, (["reports", "--json"], ["reports", "1", "--sha", sha, "--action", "open", "--json"])),
                    (ordinary+rows, (["results", "--json"], ["results", "101", "--sha", sha, "--json"], ["results", "102", "--sha", sha, "--json"]))):
    snapshot = json.dumps(dict(reports=dict(complete=True, observed=int(time.time()), reports=data)))
    status.write_text(snapshot)
    for args in calls:
        start = time.perf_counter()
        value = json.loads(run(args))
        elapsed = (time.perf_counter() - start) * 1000
        assert value["complete"] is True
        commands.append(dict(command="taskq " + " ".join(args), collected=len(data), wall_ms_including_cargo=round(elapsed, 3), outcome="ok"))
    assert status.read_text() == snapshot
events = [json.loads(line).get("observation", {}) for line in run(["profile", "export", "--since", "1h"]).splitlines()]
timings = [dict(operation=e["operation"], duration_ms=round(e["duration_us"] / 1000, 3), outcome=e.get("outcome"))
           for e in events if ".cached" in e.get("operation", "") and e.get("duration_us") is not None]
assert all(any(t["operation"].endswith(op) for t in timings) for op in ("reports.list.cached", "reports.open.cached")), "load timings missing"
result = dict(qualification="Windows isolated CLI; 9 invented reports and 10 completed tasks; no board credentials; not deployed",
              source=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=project, text=True).strip(),
              binary_sha256=hashlib.sha256((project / "target/debug/taskq.exe").read_bytes()).hexdigest(),
              loader="Cargo supplies GNU WebView2 loader search paths; bare debug binary failed 0xC0000135",
              commands=commands, cached_operations=timings, source_unchanged=True)
Path(__file__).with_name("after-cached.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result, indent=2))
