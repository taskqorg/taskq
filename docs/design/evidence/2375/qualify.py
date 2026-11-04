#!/usr/bin/env python3
"""Run one bounded native tray sequence against an instrumented `taskq` build and summarize raw logs (#2375).

Usage: qualify.py BIN SAMPLER FIXTURE_SH OUT LABEL CASE [--visual]
The target runs in snapshot mode from an isolated temporary install: no host lock, service, tray icon or ticks.
"""
import json, os, pathlib, shutil, subprocess, sys, tempfile, time

CASES = {
    # The owner's saved shape: pinned, fixed manual size, background opacity 35, font 18.
    "fixed": {"pinned": True, "size": [349.61328125, 583.6953125], "size_anchor": "fixed", "auto_anchor": "top"},
    "top": {"pinned": True, "size_anchor": "top", "auto_anchor": "top"},
    "bottom": {"pinned": True, "size_anchor": "bottom", "auto_anchor": "top"},
}
BASE = {"view": "full", "font": 18, "opacity": 100, "opacity_mode": "background", "background_opacity": 35, "backing_strength": 99}
LONG = "Invented long task title that wraps over several lines in the narrow menu at each text size"
PAGE = ("JS window.addEventListener('resize',()=>ipc.postMessage(JSON.stringify({a:'probe',e:'resize',"
        "iw:innerWidth,ih:innerHeight})))")
GEOM = ("JS ipc.postMessage(JSON.stringify({a:'probe',e:'geometry',iw:innerWidth,ih:innerHeight,"
        "scroll:document.documentElement.scrollHeight,menu:document.querySelector('#menu,body').getBoundingClientRect().height}))")


def status(root, count, long_title):
    path = root / "storefront/.taskq/status.json"
    data = json.loads(path.read_text())
    data["tasks"] = [{"n": 100 + i, "title": (LONG if long_title else f"Invented task {i}"), "state": "doing",
                      "labels": ["taskq"]} for i in range(count)]
    name = f"status-{count}-{int(long_title)}.json"
    (root / name).write_text(json.dumps(data))
    return name


def content(root, count, long_title, font, theme="dark"):
    return [f"COPY {status(root, count, long_title)} storefront/.taskq/status.json", f"STATE {theme} home {font}"]


def sequence(root):
    phase = lambda name: [f"MARK {name}"]
    steps = content(root, 3, False, 18) + [PAGE, "POS 40 40", "WAIT 3000", "HIT"]
    steps += phase("rest") + ["WAIT 15000", GEOM]
    steps += phase("content")
    for count in (12, 3, 40, 3):
        steps += content(root, count, False, 18) + ["WAIT 3000", GEOM]
    steps += phase("wrapping")
    for count, font in ((6, 18), (6, 14), (6, 20), (6, 18)):
        steps += content(root, count, True, font) + ["WAIT 3000", GEOM]
    steps += phase("straddle")
    for count in range(8, 24, 2):
        steps += content(root, count, True, 18) + ["WAIT 1500"]
    steps += phase("titlebar")
    for title in ("TaskQ title change", "TaskQ"):
        steps += [f"TITLE {title}", "WAIT 1500"]
    steps += ["VISIBLE 0", "WAIT 1000", "VISIBLE 1", "WAIT 1500"]
    steps += ["STATE light home 18", "WAIT 2500", "STATE dark home 18", "WAIT 2500", GEOM]
    steps += phase("rest-after") + content(root, 3, False, 18) + ["WAIT 15000", GEOM, "HIT", "MARK end", "EXIT"]
    return steps


def visual(root):
    steps = content(root, 3, False, 18) + [PAGE, "POS 40 40", "WAIT 2500", "HIT"]
    for theme in ("dark", "light"):
        for mode, value in (("background", 100), ("background", 35), ("whole", 80)):
            steps += [f"OPACITY {mode} {value}", f"STATE {theme} home 18", "WAIT 4000", f"SHOT {theme}-{mode}-{value}.png"]
    return steps + ["MARK end", "EXIT"]


def summarize(events, samples):
    marks = [e for e in events if str(e.get("step", "")).startswith("MARK ")]
    cpu = [s for s in samples if s["kind"] == "cpu"]
    table = []
    for a, b in zip(marks, marks[1:]):
        rows = [e for e in events if a["epoch"] <= e["epoch"] < b["epoch"]]
        inside = [c for c in cpu if a["epoch"] <= c["epoch"] < b["epoch"]]
        count = lambda kind: sum(e["kind"] == kind for e in rows)
        native = [e for e in rows if e["kind"] in ("resized", "moved")]
        steps = [s for s in samples if s["kind"] == "bounds" and a["epoch"] <= s["epoch"] < b["epoch"]]
        row = {"phase": a["step"][5:], "seconds": round(b["epoch"] - a["epoch"], 1), "resized": count("resized"), "moved": count("moved"),
               "size_requests": count("size_request"), "page_resizes": sum(e.get("e") == "resize" for e in rows),
               "bottoms_native": sorted({e["bottom"] for e in native}), "bounds_changes": len(steps),
               "bottoms_sampled": sorted({s["bottom"] for s in steps})}
        if len(inside) >= 2:
            seconds = inside[-1]["epoch"] - inside[0]["epoch"]
            row["main_pct"] = round((inside[-1]["main_s"] - inside[0]["main_s"]) / seconds * 100, 2)
            row["web_pct"] = round(sum(v - inside[0]["web_s"].get(k, v) for k, v in inside[-1]["web_s"].items()) / seconds * 100, 2)
        table.append(row)
    return table


def main():
    args = [a for a in sys.argv[1:] if a != "--visual"]
    bin_path, sampler, fixture, out, label, case = args
    out = pathlib.Path(out) / f"{label}-{case}"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp).resolve()
        subprocess.run(["sh", fixture, str(root)], check=True, env={**os.environ, "TASKQ_VERSION": "4.6.77"})
        (root / "install/tray.json").write_text(json.dumps({**BASE, **CASES[case]}))
        steps = visual(root) if "--visual" in sys.argv else sequence(root)
        script = root / "script.txt"
        script.write_text("\n".join(steps) + "\n")
        (out / "script.txt").write_text(script.read_text())
        log = out / "events.jsonl"
        env = {**os.environ, "TASKQ_INSTALL": str(root / "install"), "TASKQ_PROBE_SCRIPT": str(script), "TASKQ_PROBE_LOG": str(log)}
        for name in ("CODEX_THREAD_ID", "TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"):
            env.pop(name, None)
        target = subprocess.Popen([bin_path, "tray", "--snapshot", str(out / "shots"), "--route", "home"], cwd=root, env=env,
                                  stdout=subprocess.DEVNULL, stderr=open(out / "stderr.txt", "w"))
        with open(out / "samples.jsonl", "w") as sink:
            sampling = subprocess.Popen([sampler, str(target.pid), "300", "30", "TaskQ", str(log)], stdout=sink)
            exit_code = target.wait(timeout=300)
            sampling.terminate()
            sampling.wait()
    events = [json.loads(line) for line in log.read_text().splitlines()]
    samples = [json.loads(line) for line in (out / "samples.jsonl").read_text().splitlines()]
    report = {"label": label, "case": case, "exit": exit_code, "phases": summarize(events, samples)}
    (out / "summary.json").write_text(json.dumps(report, indent=1))
    print(json.dumps(report))


if __name__ == "__main__":
    main()
