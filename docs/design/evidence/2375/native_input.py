#!/usr/bin/env python3
"""Exercise Close, titlebar dragging, the manual grip, the Fixed choice and the copy widget with real events (#2375).

Usage: native_input.py BIN HELPER FIXTURE_SH OUT LABEL CASE
Each action posts mouse events to the target process only, except the guarded titlebar drag described in native-input.swift.
"""
import json, os, pathlib, shutil, subprocess, sys, tempfile, time

from PIL import Image

from qualify import BASE, CASES, PAGE, content

RECTS = ("JS {const r=q=>{const e=document.querySelector(q);if(!e)return null;const b=e.getBoundingClientRect();"
         "return [b.left,b.top,b.width,b.height]};ipc.postMessage(JSON.stringify({a:'probe',e:'rects',grip:r('.grip'),"
         "copy:r('[data-copy-label]'),value:document.querySelector('[data-copy-label]')?.dataset.copyLabel||null,"
         "iw:innerWidth,ih:innerHeight}))}")
TOP = "JS ipc.postMessage(JSON.stringify({a:'top'}))"
ANCHOR = "JS ipc.postMessage(JSON.stringify({a:'size-anchor',value:'fixed'}))"
PHASES = ("drag", "grip", "copy", "close")


def rows(log):
    # The target may be writing the last line while this reads it.
    return [json.loads(line) for line in log.read_text().splitlines() if line.endswith("}")] if log.exists() else []


def wait_mark(log, name, limit=30):
    end = time.time() + limit
    while time.time() < end:
        found = [e for e in rows(log) if e.get("step") == f"MARK {name}"]
        if found:
            return found[0]
        time.sleep(0.05)
    raise SystemExit(f"no mark {name}")


def main():
    bin_path, helper, fixture, out, label, case = sys.argv[1:]
    out = pathlib.Path(out) / f"{label}-native-{case}"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    report = {"label": label, "case": case, "actions": {}}
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp).resolve()
        subprocess.run(["sh", fixture, str(root)], check=True, env={**os.environ, "TASKQ_VERSION": "4.6.77"})
        settings = root / "install/tray.json"
        settings.write_text(json.dumps({**BASE, **CASES[case]}))
        steps = content(root, 3, False, 18) + [PAGE, "POS 40 40", TOP, "WAIT 3000", "HIT", RECTS, "WAIT 500"]
        for phase in PHASES:
            steps += [f"MARK {phase}", "WAIT 8000", "HIT", RECTS, "WAIT 500"]
            if phase == "grip":
                steps += [ANCHOR, "WAIT 500", RECTS, "WAIT 500"]
        steps += ["MARK end", "EXIT"]
        script = root / "script.txt"
        script.write_text("\n".join(steps) + "\n")
        (out / "script.txt").write_text(script.read_text())
        log = out / "events.jsonl"
        env = {**os.environ, "TASKQ_INSTALL": str(root / "install"), "TASKQ_PROBE_SCRIPT": str(script), "TASKQ_PROBE_LOG": str(log)}
        for name in ("CODEX_THREAD_ID", "TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT"):
            env.pop(name, None)
        target = subprocess.Popen([bin_path, "tray", "--snapshot", str(out / "shots"), "--route", "home"], cwd=root, env=env,
                                  stdout=subprocess.DEVNULL, stderr=open(out / "stderr.txt", "w"))

        def window():
            return [e for e in rows(log) if e["kind"] == "hit"][-1]["window"]

        def act(*args):
            done = subprocess.run([helper, str(target.pid), str(window()), *map(str, args)], capture_output=True, text=True)
            return done.stdout.strip() or f"exit {done.returncode}"

        def bounds():
            box = json.loads(act("bounds"))
            return box["X"], box["Y"], box["Width"], box["Height"]

        def titlebar_alpha(box):
            # The window server passes clicks through fully transparent pixels to whatever window lies below.
            shot = root / "titlebar.png"
            subprocess.run(["screencapture", "-x", "-o", "-l", str(window()), str(shot)], check=True)
            image = Image.open(shot).convert("RGBA")
            scale = image.width / box[2]
            return image.getpixel((int(box[2] / 2 * scale), int(14 * scale)))[3]

        def page():
            return [e for e in rows(log) if e.get("e") == "rects"][-1]

        def center(rect, box, view):
            left, top, width, height = rect
            return box[0] + left + width / 2, box[1] + box[3] - view["ih"] + top + height / 2

        try:
            wait_mark(log, "drag")
            time.sleep(0.5)
            box = bounds()
            alpha = titlebar_alpha(box)
            guard = {"titlebar_alpha": alpha, "performed": False}
            if alpha >= 128:
                guard.update(json.loads(act("hid-drag", box[0] + box[2] / 2, box[1] + 14, box[0] + box[2] / 2 + 60, box[1] + 54)))
            time.sleep(2)
            report["actions"]["drag"] = {**guard, "before": box, "after": bounds()}

            wait_mark(log, "grip")
            time.sleep(0.5)
            box, view = bounds(), page()
            if view["grip"]:
                x, y = center(view["grip"], box, view)
                act("drag", x, y, x + 50, y + 60)
            time.sleep(2)
            report["actions"]["grip"] = {"found": bool(view["grip"]), "before": box, "after": bounds()}

            wait_mark(log, "copy")
            time.sleep(0.5)
            box, view = bounds(), page()
            saved = json.loads(settings.read_text())
            report["actions"]["anchor"] = {"window": box, "client": [view["iw"], view["ih"]], "size_anchor": saved.get("size_anchor"), "saved_size": saved.get("size")}
            if view["copy"]:
                x, y = center(view["copy"], box, view)
                report["actions"]["copy"] = {"expected": view["value"], **json.loads(act("copy", x, y))}
            else:
                report["actions"]["copy"] = {"found": False}

            wait_mark(log, "close")
            time.sleep(0.5)
            box = bounds()
            hit = [e for e in rows(log) if e["kind"] == "hit"][-1]
            act("click", box[0] + hit["close_center"][0], box[1] + hit["close_center"][1])
            time.sleep(2)
            report["actions"]["close"] = {"close_requested": [e for e in rows(log) if e["kind"] == "close_requested"] != []}
            target.wait(timeout=60)
        finally:
            if target.poll() is None:
                target.kill()
    events = rows(log)
    count = lambda kind: sum(e["kind"] == kind for e in events)
    report["events"] = {kind: count(kind) for kind in ("resized", "moved", "close_requested", "focused", "unfocused", "size_request")}
    report["hits"] = [{k: e.get(k) for k in ("hit_close", "hit_bar", "bar_can_move_window", "web_pid") if k != "web_pid"} for e in events if e["kind"] == "hit"][:1]
    (out / "summary.json").write_text(json.dumps(report, indent=1))
    print(json.dumps(report, indent=1))


if __name__ == "__main__":
    main()
