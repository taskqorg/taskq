#!/usr/bin/env python3
"""Bounded live scenario (docs/runtime-qualification.md) for an unmerged candidate: a temporary install and fixture
root that the ordinary host service never reads. PM-only; the files it writes stay local and private.

  live-probe.py prepare --exe CANDIDATE --from-install INSTALL --repo CHECKOUT --name HOST DIR
  live-probe.py report DIR
  live-probe.py env DIR        # print the env assignments for this fixture

`prepare` copies the board token, grants (worker-policy.json) and accounts.json from INSTALL and writes a host.json
with `claim: labelled` and one slot per runtime, so the candidate takes only tasks labelled `host-HOST`. INSTALL is
read, never written. Nothing is installed as a unit; run the candidate in the foreground and stop it with Ctrl-C.

PM steps per host:
1. Build the exact candidate; `prepare` into a new temp dir; load the `env` assignments. On a Claude trust refusal,
   run `claude` once in the fixture root and accept the prompt.
2. `<candidate> service run` in the foreground. A run outside an install never updates itself; ordinary services
   skip `host-HOST` tasks.
3. Add held fixture tasks with `--host HOST` per scenario steps 2-5 and start them. Q1: one per runtime. Q15 (Claude
   task): stop the service mid-launch, start it again, check stop readback, same ID on resume, Remote Control line.
   Q6: `<candidate> service setup --target <fixture root>`, continuation from Conversations.
4. `report`; link candidate SHA, client versions and session IDs in the task; keep paths and report.json local.
   Stop the service, close the fixture tasks, delete the temp dir."""
import argparse, json, os, platform, shutil, socket, subprocess, sys

COPY = ["token", "gitlab-token", "worker-policy.json", "accounts.json"]
KEEP = ["permission_mode", "codex", "has", "default_runtime", "channel"]


def env(d):
    m = json.load(open(os.path.join(d, "probe.json")))
    return {"TASKQ_INSTALL": os.path.join(d, "install"), "TASKQ_HOST": m["name"]}, m


def run(cmd, extra=None):
    e = {k: v for k, v in os.environ.items() if k not in ("TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT", "CODEX_THREAD_ID")}
    e.update(extra or {})
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=e, timeout=120)
        return (p.stdout or p.stderr).strip()
    except (OSError, subprocess.TimeoutExpired) as x:
        return f"unknown ({x})"


def prepare(a):
    d = os.path.abspath(a.dir)
    if os.path.exists(d):
        sys.exit(f"{d} exists; pick a new folder")
    inst, root = os.path.join(d, "install"), os.path.join(d, "root")
    os.makedirs(inst)
    for n in COPY:
        src = os.path.join(a.from_install, n)
        if os.path.isfile(src):
            shutil.copyfile(src, os.path.join(inst, n))
            os.chmod(os.path.join(inst, n), 0o600)
    real = json.load(open(os.path.join(a.from_install, "host.json")))
    host = {k: real[k] for k in KEEP if k in real}
    host.update({"hosts": {socket.gethostname(): a.name}, "claim": "labelled", "limits": {"claude": 1, "codex": 1, "codex-app": 1}})
    json.dump(host, open(os.path.join(inst, "host.json"), "w"), indent=1)
    origin = run(["git", "-C", a.repo, "remote", "get-url", "origin"])
    subprocess.run(["git", "clone", "--quiet", a.repo, root], check=True)
    subprocess.run(["git", "-C", root, "remote", "set-url", "origin", origin], check=True)
    open(os.path.join(inst, "projects"), "w").write(root + "\n")
    # the checkout's existing grants and untracked runtime settings, mirrored onto the fixture root; nothing new is granted
    pol = os.path.join(inst, "worker-policy.json")
    if os.path.isfile(pol):
        p = json.load(open(pol))
        same = [g for g in p.get("grants", []) if os.path.normcase(os.path.realpath(g.get("project") or "")) == os.path.normcase(os.path.realpath(a.repo))]
        p["grants"] = p.get("grants", []) + [dict(g, project=os.path.realpath(root)) for g in same]
        json.dump(p, open(pol, "w"), indent=1)
    for f in [os.path.join(".claude", "settings.local.json"), os.path.join(".codex", "config.toml")]:
        if os.path.isfile(os.path.join(a.repo, f)):
            os.makedirs(os.path.dirname(os.path.join(root, f)), exist_ok=True)
            shutil.copyfile(os.path.join(a.repo, f), os.path.join(root, f))
    exe = os.path.abspath(a.exe)
    json.dump({"name": a.name, "exe": exe, "root": root, "sha": run(["git", "-C", a.repo, "rev-parse", "HEAD"])},
              open(os.path.join(d, "probe.json"), "w"), indent=1)
    print(f"fixture ready; host {a.name}, label host-{a.name}")


def report(a):
    d = os.path.abspath(a.dir)
    e, m = env(d)
    r = {"candidate": m["sha"], "version": run([m["exe"], "--version"], e), "os": platform.platform(),
         "claude": run(["claude", "--version"]), "codex": run(["codex", "--version"]),
         "status": run([m["exe"], "service", "status", "--json"], e)}
    json.dump(r, open(os.path.join(d, "report.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in r.items() if k != "status"}, indent=1))


def show(a):
    e, _ = env(os.path.abspath(a.dir))
    for k, v in e.items():
        print(f"$env:{k}='{v}'" if os.name == "nt" else f"export {k}='{v}'")


p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
s = p.add_subparsers(dest="cmd", required=True)
q = s.add_parser("prepare")
q.add_argument("--exe", required=True)
q.add_argument("--from-install", required=True)
q.add_argument("--repo", required=True)
q.add_argument("--name", required=True)
q.add_argument("dir")
s.add_parser("report").add_argument("dir")
s.add_parser("env").add_argument("dir")
a = p.parse_args()
{"prepare": prepare, "report": report, "env": show}[a.cmd](a)
