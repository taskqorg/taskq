#!/usr/bin/env python3
"""UI bench (docs/testing.md): hold the tray page of two builds on the same fixed fixtures and compare them.

Each run starts `taskq tray --snapshot DIR --route bench` on a generated install, samples CPU time and memory of
the taskq process and its webview processes once a second, then reads the page samples from `bench.json`. A value that
was not observed prints `unknown`, never 0.

  python3 scripts/ui-bench.py                         # base origin/main vs this checkout, both built here
  python3 scripts/ui-bench.py --base-bin A --candidate-bin B --hold 60
"""
import argparse, ctypes, json, math, os, platform, shutil, struct, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WINDOWS = platform.system() == "Windows"
EXE = "taskq.exe" if WINDOWS else "taskq"
STATES = ["doing", "review", "ask", "ready", "ready", "waiting", "held", "failed"]


def fixture(d, projects, tasks, history):
    """A fixed install: `projects` projects of `tasks` tasks each and `history` AI requests; same bytes every run but times."""
    install = d / "install"
    install.mkdir(parents=True)
    now, roots = int(time.time()), []
    for p in range(projects):
        root = d / f"project-{p:02}"
        (root / ".taskq").mkdir(parents=True)
        roots.append(str(root))
        rows = [{"n": 100 * p + i, "title": f"Task {i} of project {p}: keep the cart after sign-in on every device",
                 "state": STATES[i % len(STATES)], "labels": ["taskq", f"area-{i % 3}"]} for i in range(tasks)]
        for r in rows:
            if r["state"] == "ask":
                r.update(revision=r["n"], question={"parsed": True, "done": "Cart merge reaches duplicate products.",
                    "problem": "Which quantity should a duplicate product keep?", "evidence": [], "choices": [
                        {"id": 1, "text": "Keep the larger quantity", "recommended": True, "consequential": False},
                        {"id": 2, "text": "Add the quantities", "recommended": False, "consequential": False}]})
        status = {"project": f"project-{p:02}", "host": "bench", "version": "4.6.0", "issues": f"https://github.com/acme/p{p}/issues",
                  "log": None, "focus": ["area-0"], "error": None, "request": None, "events_since": now - 86400, "tasks": rows}
        (root / ".taskq" / "status.json").write_text(json.dumps(status))
        (root / ".taskq" / "events").write_text(f"since {now - 86400}\n" + "".join(f"{now - 3000 * k} #{k} close {k}\n" for k in range(1, 9)))
    (install / "projects").write_text("\n".join(roots) + "\n")
    text = "Explain why the review of the checkout task failed and propose the smallest fix that keeps every existing check"
    (install / "tray-requests.json").write_text(json.dumps({"draft": "", "history": [f"{k}. {text}" for k in range(history)]}))
    return install


FIXTURES = {"small": (2, 4, 2), "large": (12, 25, 20)}


def processes():
    """pid -> (parent or None, cpu seconds, memory bytes, name) for every process this user can read."""
    out = {}
    if WINDOWS:
        ps = "Get-CimInstance Win32_Process | % { \"$($_.ProcessId) $($_.ParentProcessId) $($_.UserModeTime + $_.KernelModeTime) $($_.WorkingSetSize) $($_.Name)\" }"
        text = subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True).stdout
        for line in text.splitlines():
            f = line.split(None, 4)
            if len(f) == 5 and f[0].isdigit():
                out[int(f[0])] = (int(f[1]), int(f[2] or 0) / 1e7, int(f[3] or 0), f[4])
        return out
    # macOS: libproc, since a sandboxed agent may not run the setuid `ps`; memory is the physical footprint Activity Monitor shows
    lib, name, ru = ctypes.CDLL("/usr/lib/libproc.dylib"), ctypes.create_string_buffer(256), ctypes.create_string_buffer(512)
    tb = (ctypes.c_uint32 * 2)()
    ctypes.CDLL(None).mach_timebase_info(tb)
    n = lib.proc_listallpids(None, 0)
    pids = (ctypes.c_int * (2 * n))()
    n = lib.proc_listallpids(pids, ctypes.sizeof(pids))
    for p in pids[:n]:
        if lib.proc_pid_rusage(p, 2, ru) == 0:  # RUSAGE_INFO_V2: user, system at 16; phys_footprint at 72
            user, system = struct.unpack_from("QQ", ru, 16)
            lib.proc_name(p, name, 256)
            out[p] = (None, (user + system) * tb[0] / tb[1] / 1e9, struct.unpack_from("Q", ru, 72)[0], name.value.decode(errors="replace"))
    return out


def tree(ps, pid, before):
    """taskq and its webview processes: descendants on Windows (WebView2); on macOS WebKit XPC processes started since launch."""
    # ponytail: on macOS WebKit processes are launchd children, so any new com.apple.WebKit process of this user counts;
    # another WebKit app started during the hold inflates the numbers. Read the responsible pid if that matters.
    mine = {pid}
    if WINDOWS:
        grew = True
        while grew:
            kids = {p for p, v in ps.items() if v[0] in mine} - mine
            grew, mine = bool(kids), mine | kids
    else:
        mine |= {p for p, v in ps.items() if p not in before and "com.apple.WebKit" in v[3]}
    return {p: ps[p] for p in mine if p in ps}


def run(binary, install, hold, gap, out):
    shutil.rmtree(out, ignore_errors=True)
    env = dict(os.environ, TASKQ_INSTALL=str(install), TASKQ_BENCH_HOLD=str(hold), TASKQ_BENCH_GAP=str(gap))
    for k in ("TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT", "CODEX_THREAD_ID"):
        env.pop(k, None)
    before = set(processes())
    child = subprocess.Popen([str(binary), "tray", "--snapshot", str(out), "--route", "bench"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    samples, last, deadline = [], None, time.time() + hold + 90
    while child.poll() is None and time.time() < deadline:
        t, procs = time.time(), tree(processes(), child.pid, before)
        if child.pid not in procs:  # exiting: its webview processes are already going
            break
        # per process, so a process that ends between samples does not subtract its CPU time
        if last:
            cpu = sum(v[1] - last[1].get(p, 0) for p, v in procs.items())
            samples.append({"cpu": 100 * cpu / (t - last[0]), "mem": sum(v[2] for v in procs.values())})
        last = (t, {p: v[1] for p, v in procs.items()})
        time.sleep(1)
    if child.poll() is None:
        child.kill()
    err = child.communicate()[1].strip()
    try:
        bench = json.loads((out / "bench.json").read_text())
    except (OSError, ValueError) as e:
        bench = {"failure": f"bench.json: {e}"}
    return {"exit": child.returncode, "stderr": err[-400:], "bench": bench, "samples": samples}


def pct(v, q):
    """Nearest-rank percentile; None for no samples."""
    v = sorted(v)
    return v[max(0, math.ceil(q / 100 * len(v)) - 1)] if v else None


def summary(r, hold):
    page, s = r["bench"].get("page") or {}, r["samples"]
    # the sustained hold excludes the first 5 s of start-up
    held = s[5:] if len(s) > 10 else []
    ok = r["exit"] == 0 and page and not page.get("failure")
    row = {"ok": bool(ok), "status": "ok" if ok else (r["bench"].get("failure") or page.get("failure") or f"exit {r['exit']}: {r['stderr']}")}
    for k in ("snapshot", "typing", "input"):
        for q in (95, 99):
            row[f"{k}_p{q}_ms"] = pct(page.get(k, []), q) if ok else None
        row[f"{k}_n"] = len(page.get(k, [])) if ok else None
    row["errors"] = (page.get("errors", 0) + page.get("stalls", 0) + page.get("lost", 0) + int(bool(r["bench"].get("web_failed")))) if ok else None
    # CPU percent of one core: 100 is one busy core; divide by `cores` for the share of the host
    row["cpu_mean_pct"] = sum(x["cpu"] for x in held) / len(held) if held else None
    row["cpu_p95_pct"] = pct([x["cpu"] for x in held], 95)
    row["cores"] = os.cpu_count()
    row["mem_peak_mb"] = max(x["mem"] for x in s) / 2**20 if s else None
    row["mem_end_mb"] = s[-1]["mem"] / 2**20 if s else None
    row["js_heap_mb"] = page["heap"] / 2**20 if ok and page.get("heap") else None
    return row


def build(ref):
    """Build `ref` in its own worktree and target folder under target/ui-bench; returns the binary."""
    src, target = ROOT / "target" / "ui-bench" / "base-src", ROOT / "target" / "ui-bench" / "base-target"
    subprocess.run(["git", "worktree", "remove", "--force", str(src)], cwd=ROOT, capture_output=True)
    subprocess.run(["git", "worktree", "add", "--detach", str(src), ref], cwd=ROOT, check=True)
    subprocess.run(["npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund"], cwd=src, check=True, shell=WINDOWS)
    subprocess.run(["cargo", "build", "--release", "--locked"], cwd=src, check=True, env=dict(os.environ, CARGO_TARGET_DIR=str(target)))
    return target / "release" / EXE


def fmt(v):
    return "unknown" if v is None else f"{v:.1f}" if isinstance(v, float) else str(v)


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument("--base", default="origin/main", help="git ref built as the baseline (default origin/main)")
    a.add_argument("--base-bin", help="baseline binary instead of building --base")
    a.add_argument("--candidate-bin", help="candidate binary (default: cargo build --release of this checkout)")
    a.add_argument("--hold", type=int, default=60, help="seconds per run (default 60)")
    a.add_argument("--gap", type=int, default=250, help="ms between page steps (default 250)")
    a.add_argument("--fixture", choices=list(FIXTURES), action="append", help="default: small and large")
    a.add_argument("--out", default=str(ROOT / "target" / "ui-bench"), help="results folder")
    o = a.parse_args()
    out = Path(o.out)
    cand = Path(o.candidate_bin) if o.candidate_bin else None
    if not cand:
        subprocess.run(["cargo", "build", "--release", "--locked"], cwd=ROOT, check=True)
        cand = ROOT / "target" / "release" / EXE
    base = Path(o.base_bin) if o.base_bin else build(o.base)
    results = {"host": platform.platform(), "machine": platform.machine(), "cores": os.cpu_count(), "hold_s": o.hold, "gap_ms": o.gap,
               "fixtures": {k: dict(zip(("projects", "tasks_each", "ai_history"), FIXTURES[k])) for k in FIXTURES}, "runs": {}}
    for name in o.fixture or list(FIXTURES):
        for role, binary in (("base", base), ("candidate", cand)):
            d = out / f"{name}-{role}"
            shutil.rmtree(d, ignore_errors=True)
            install = fixture(d / "fx", *FIXTURES[name])
            r = run(binary.resolve(), install, o.hold, o.gap, d / "shots")
            results["runs"][f"{name}/{role}"] = {"binary": str(binary), "summary": summary(r, o.hold), "raw": r}
            print(f"{name}/{role}: {'ok' if r['exit'] == 0 else 'failed'}", file=sys.stderr)
    (out / "results.json").write_text(json.dumps(results, indent=1))
    keys = [k for k in next(iter(results["runs"].values()))["summary"] if k not in ("ok", "cores")]
    runs = list(results["runs"])
    print(f"UI bench, {results['host']}, {results['cores']} cores, hold {o.hold} s, gap {o.gap} ms\n")
    print("| Measure | " + " | ".join(runs) + " |\n|---|" + "---|" * len(runs))
    for k in keys:
        print(f"| {k} | " + " | ".join(fmt(results["runs"][r]["summary"][k]) for r in runs) + " |")
    print(f"\nraw: {out / 'results.json'}")
    return 0 if all(r["summary"]["ok"] for r in results["runs"].values()) else 1


if __name__ == "__main__":
    sys.exit(main())
