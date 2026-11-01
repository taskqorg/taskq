#!/usr/bin/env python3
"""Bounded live scenario (docs/runtime-qualification.md) for an unmerged candidate, PM-only, one host at a time.
The candidate runs from an owned 0700 fixture with its own install and root; the ordinary install, its service,
grants, trust and client configuration are read, never written. Every file this script writes stays local.

  live-probe.py identity --repo CHECKOUT --from-install INSTALL --parent DIR      read-only host and access probe
  live-probe.py build --sha SHA --repo CHECKOUT --out DIR [--minutes M]     one-job candidate build in an owned clean clone
  live-probe.py prepare --build DIR --repo CHECKOUT --from-install INSTALL --parent DIR --name HOST
  live-probe.py tasks FIXTURE            admission check, then held add, readback and start by op id (label host-HOST)
  live-probe.py run FIXTURE [--minutes M] [--interrupt-on REGEX]    supervised candidate service, killed at deadline
  live-probe.py tray FIXTURE [--minutes M]                          supervised candidate app on the fixture install
  live-probe.py report FIXTURE           receipts, digests and the unchanged-state proof in FIXTURE/report.json
  live-probe.py cleanup FIXTURE          close fixture tasks, stop fixture runtimes, archive receipts, delete FIXTURE

`build` clones CHECKOUT into DIR/src (DIR must be new; the dirty primary checkout is only read), checks out SHA,
runs `cargo build --release --locked -j 1` under a deadline and writes DIR/build.json: SHA, toolchain, binary path
and SHA-256. `prepare` writes FIXTURE/RUNBOOK.txt with this host's exact commands. It refuses when the build source
is not exactly SHA and clean, when the binary digest differs from build.json or it reports no version, or when no
existing Claude trust entry covers PARENT: it never changes trust, login or grants. PARENT must already sit under a trusted root, such as the checkout's `.worktrees`.
It also refuses a HOST name that any board heartbeat already carries, so no ordinary host can admit a fixture task.
`prepare` and `run` first reconcile earlier probes: they refuse while another `probe-*` fixture folder exists under
PARENT, while any process names one, or while an open board task titled `probe 2094` is listed; earlier
`*-receipts.tar.gz` archives are listed and kept. Cleanup ownership: the probe ends only processes whose command
line names its own fixture folder and Claude jobs whose folder is inside it; ordinary services and jobs never match.

Why the ordinary host stays untouched: the fixture install has `claim: labelled` and its own host name, so only
`host-HOST` tasks reach it and ordinary services skip them; a run outside an install never updates itself.
`service setup --target` only writes `<install>/setup/<id>.json`, and the launch writes only `brief.txt` and
`launch.log` under it; trust is written only by `service onboard` and grants only by `service permissions`, which
the probe never runs. The tray's one-per-host lock lives in the OS temp folder, so `tray` gives the candidate app
its own temp folder; its Conversations act on TASKQ_INSTALL, the fixture install. `report` compares digests of the
ordinary install files, client settings, the trusted-folder list and the checkout's focus and runtime settings
against the `prepare` snapshot and names each one unchanged or CHANGED."""
import argparse, hashlib, json, os, platform, re, shutil, signal, socket, subprocess, sys, tarfile, time

COPY = ["token", "gitlab-token", "worker-policy.json", "accounts.json"]
KEEP = ["permission_mode", "codex", "has", "default_runtime", "channel"]
STRIP = ("TASKQ_TASK", "TASKQ_ATTEMPT", "TASKQ_ROLE", "TASKQ_ROOT", "TASKQ_INSTALL", "TASKQ_HOST", "CODEX_THREAD_ID")
LIVE = ("running", "working", "idle", "waiting")
HOME = os.path.expanduser("~")
WIN = os.name == "nt"


def sha256(p):
    try:
        return hashlib.sha256(open(p, "rb").read()).hexdigest()
    except OSError:
        return None


def run(cmd, extra=None, cwd=None, timeout=120):
    e = {k: v for k, v in os.environ.items() if k not in STRIP}
    e.update(extra or {})
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, env=e, cwd=cwd, timeout=timeout)
        return p.returncode, (p.stdout + p.stderr).strip()
    except (OSError, subprocess.TimeoutExpired) as x:
        return None, f"unknown ({x})"


def key(p):
    p = os.path.realpath(p).replace("\\", "/")
    return p.lower() if WIN else p


def trusted(parent):
    """Existing ~/.claude.json trust entries covering PARENT (Claude walks parent folders) and all trusted keys; read-only."""
    try:
        projects = json.load(open(os.path.join(HOME, ".claude.json"), encoding="utf-8")).get("projects", {})
    except (OSError, ValueError) as x:
        return f"unknown ({x})", None
    keys = sorted(k for k, v in projects.items() if v.get("hasTrustDialogAccepted"))
    want = key(parent) + "/"
    return [k for k in keys if want.startswith(key(k.removeprefix("//?/")).rstrip("/") + "/")], keys


def snapshot(m):
    """Digests of everything the probe must leave unchanged; contents never reach the report."""
    src, repo = m["from_install"], m["repo"]
    files = {f"install/{n}": os.path.join(src, n) for n in ["host.json", "projects", *COPY]}
    files.update({"claude/settings.json": os.path.join(HOME, ".claude", "settings.json"),
                  "codex/config.toml": os.path.join(HOME, ".codex", "config.toml"),
                  "checkout/.taskq/focus": os.path.join(repo, ".taskq", "focus"),
                  "checkout/.claude/settings.local.json": os.path.join(repo, ".claude", "settings.local.json"),
                  "checkout/.codex/config.toml": os.path.join(repo, ".codex", "config.toml")})
    out = {k: sha256(p) for k, p in files.items()}
    _, keys = trusted(m["parent"])
    out["claude trusted folders"] = hashlib.sha256(json.dumps(keys).encode()).hexdigest() if keys is not None else None
    return out


def load(d):
    d = os.path.abspath(d)
    m = json.load(open(os.path.join(d, "probe.json")))
    if m["dir"] != d:
        sys.exit("probe.json belongs to another folder")
    return d, m, {"TASKQ_INSTALL": os.path.join(d, "install"), "TASKQ_HOST": m["name"]}


def save(d, m):
    json.dump(m, open(os.path.join(d, "probe.json"), "w"), indent=1)


def reconcile(parent, repo, own=None):
    """Earlier probe leftovers that must be resolved (by their own `cleanup`) before a fixture is made or launched."""
    bad, mine = [], key(own) if own else None
    for f in sorted(os.listdir(parent)) if os.path.isdir(parent) else []:
        p = os.path.join(parent, f)
        if f.startswith("probe-") and f.endswith("-receipts.tar.gz"):
            print("kept receipts", p)
        elif f.startswith("probe-") and os.path.isdir(p) and key(p) != mine:
            bad.append(f"fixture {p} exists; run its `cleanup` first")
    bad += [f"process {pid} names an earlier fixture: {cmd[:120]}" for pid, cmd in procs({"dir": os.path.join(parent, "probe-")})
            if not (own and norm(own) in norm(cmd))]
    try:
        rows = json.load(open(os.path.join(repo, ".taskq", "status.json"))).get("tasks", [])
    except (OSError, ValueError) as x:
        return bad + [f"board task list unknown ({x})"]
    own_n = set()
    if own:
        own_n = {t.get("n") for t in json.load(open(os.path.join(own, "probe.json"))).get("tasks", {}).values()}
    bad += [f"open board task #{t.get('n')} {t.get('title')}; close it first" for t in rows
            if str(t.get("title", "")).startswith("probe 2094") and str(t.get("state")).lower() != "closed" and t.get("n") not in own_n]
    return bad


def norm(s):
    return s.replace("\\", "/").lower() if WIN else s


def build(a):
    out = os.path.abspath(a.out)
    os.makedirs(out, mode=0o700)
    src, target = os.path.join(out, "src"), os.path.join(out, "target")
    subprocess.run(["git", "clone", "--quiet", "--no-hardlinks", "--no-checkout", os.path.abspath(a.repo), src], check=True)
    if run(["git", "-C", src, "cat-file", "-e", a.sha + "^{commit}"])[0] != 0:
        _, origin = run(["git", "-C", a.repo, "remote", "get-url", "origin"])
        subprocess.run(["git", "-C", src, "fetch", "--quiet", origin, a.sha], check=True)
    subprocess.run(["git", "-C", src, "checkout", "--quiet", "--detach", a.sha], check=True)
    started = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    e = {k: v for k, v in os.environ.items() if k not in STRIP}
    e["CARGO_TARGET_DIR"] = target
    with open(os.path.join(out, "build.log"), "w") as log:
        p = subprocess.Popen(["cargo", "build", "--release", "--locked", "-j", "1", "--bin", "taskq"], cwd=src, env=e,
                             stdout=log, stderr=subprocess.STDOUT, start_new_session=not WIN)
        try:
            rc = p.wait(a.minutes * 60)
        except subprocess.TimeoutExpired:
            kill_tree(p)
            rc = "deadline"
    exe = os.path.join(target, "release", "taskq.exe" if WIN else "taskq")
    _, head = run(["git", "-C", src, "rev-parse", "HEAD"])
    _, dirty = run(["git", "-C", src, "status", "--porcelain", "--untracked-files=no"])
    r = {"sha": a.sha, "head": head, "clean": not dirty, "exit": rc, "started": started,
         "finished": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "rustc": run(["rustc", "-V"])[1],
         "cargo": run(["cargo", "-V"])[1], "src": src, "exe": exe, "exe_sha256": sha256(exe) if rc == 0 else None}
    json.dump(r, open(os.path.join(out, "build.json"), "w"), indent=1)
    print(json.dumps(r, indent=1))
    if rc != 0 or head != a.sha or dirty:
        sys.exit("build failed or source not clean at SHA; see build.log")


def identity(a):
    _, cv = run(["claude", "--version"])
    _, xv = run(["codex", "--version"])
    _, xl = run(["codex", "login", "status"])
    hits, _ = trusted(a.parent)
    try:
        grants = [(g.get("runtime"), g.get("mode")) for g in json.load(open(os.path.join(a.from_install, "worker-policy.json"))).get("grants", [])
                  if key(g.get("project") or "") == key(a.repo)]
    except (OSError, ValueError) as x:
        grants = f"unknown ({x})"
    try:
        mode = json.load(open(os.path.join(a.from_install, "host.json"))).get("permission_mode")
    except (OSError, ValueError) as x:
        mode = f"unknown ({x})"
    print(json.dumps({"hostname": socket.gethostname(), "user": os.environ.get("USER") or os.environ.get("USERNAME"), "os": platform.platform(),
                      "python": sys.version.split()[0], "claude": cv, "claude path": shutil.which("claude"), "codex": xv,
                      "codex path": shutil.which("codex"), "codex login": xl.splitlines()[-1] if xl else "unknown",
                      "board token file": os.path.isfile(os.path.join(a.from_install, "token")), "claude trust covering parent": hits,
                      "checkout grants": grants, "permission_mode": mode}, indent=1))


def prepare(a):
    parent, repo, src = os.path.abspath(a.parent), os.path.abspath(a.repo), os.path.abspath(a.from_install)
    b = json.load(open(os.path.join(os.path.abspath(a.build), "build.json")))
    a.sha, a.exe = b["sha"], b["exe"]
    _, head = run(["git", "-C", b["src"], "rev-parse", "HEAD"])
    _, dirty = run(["git", "-C", b["src"], "status", "--porcelain", "--untracked-files=no"])
    if head != a.sha or dirty or b.get("exit") != 0:
        sys.exit(f"build source is {head}{' with changes' if dirty else ''}, exit {b.get('exit')}, not a clean {a.sha} build")
    if sha256(a.exe) != b["exe_sha256"]:
        sys.exit("candidate binary differs from build.json")
    bad = reconcile(parent, repo)
    if bad:
        sys.exit("earlier probe not reconciled, nothing created:\n" + "\n".join(bad))
    rc, version = run([a.exe, "--version"])
    if rc != 0:
        sys.exit(f"candidate --version failed: {version}")
    hits, _ = trusted(parent)
    if not isinstance(hits, list) or not hits:
        sys.exit(f"no existing Claude trust entry covers the parent folder ({hits}); trust is never changed here")
    # a `host-NAME` task is admitted only by the host of that name, so NAME must be new to every heartbeat on the board
    try:
        st = json.load(open(os.path.join(repo, ".taskq", "status.json")))
        known = {h.get("name") for h in st.get("hosts", [])} | {st.get("host")}
    except (OSError, ValueError) as x:
        sys.exit(f"cannot read the board's host names from the checkout status ({x}); fixture name unverified")
    if a.name in known:
        sys.exit(f"host name {a.name} already heartbeats on the board; an ordinary host would admit the fixture tasks")
    d = os.path.join(parent, f"probe-{a.name}-{int(time.time())}")
    os.makedirs(d, mode=0o700)
    os.chmod(d, 0o700)
    for sub in ["install", "bin", "logs", "tmp", "q6"]:
        os.makedirs(os.path.join(d, sub), mode=0o700)
    inst, root = os.path.join(d, "install"), os.path.join(d, "root")
    exe = os.path.join(d, "bin", os.path.basename(a.exe))
    shutil.copy2(a.exe, exe)
    for n in COPY:
        if os.path.isfile(os.path.join(src, n)):
            shutil.copyfile(os.path.join(src, n), os.path.join(inst, n))
            os.chmod(os.path.join(inst, n), 0o600)
    real = json.load(open(os.path.join(src, "host.json")))
    host = {k: real[k] for k in KEEP if k in real}
    host.update({"hosts": {socket.gethostname(): a.name}, "claim": "labelled", "limits": {"claude": 1, "codex": 1, "codex-app": 1}})
    json.dump(host, open(os.path.join(inst, "host.json"), "w"), indent=1)
    _, origin = run(["git", "-C", repo, "remote", "get-url", "origin"])
    subprocess.run(["git", "clone", "--quiet", b["src"], root], check=True)
    subprocess.run(["git", "-C", root, "remote", "set-url", "origin", origin], check=True)
    os.makedirs(os.path.join(root, ".taskq"), exist_ok=True)
    open(os.path.join(root, ".taskq", "focus"), "w").write("all\n")
    open(os.path.join(inst, "projects"), "w").write(root + "\n")
    # the checkout's existing grants and untracked runtime settings, mirrored onto the fixture root; nothing new is granted
    pol = os.path.join(inst, "worker-policy.json")
    if os.path.isfile(pol):
        p = json.load(open(pol))
        same = [g for g in p.get("grants", []) if key(g.get("project") or "") == key(repo)]
        p["grants"] = p.get("grants", []) + [dict(g, project=os.path.realpath(root)) for g in same]
        json.dump(p, open(pol, "w"), indent=1)
    for f in [os.path.join(".claude", "settings.local.json"), os.path.join(".codex", "config.toml")]:
        if os.path.isfile(os.path.join(repo, f)):
            os.makedirs(os.path.dirname(os.path.join(root, f)), exist_ok=True)
            shutil.copyfile(os.path.join(repo, f), os.path.join(root, f))
    m = {"dir": d, "name": a.name, "sha": a.sha, "exe": exe, "exe_sha256": sha256(exe), "build": b, "version": version, "root": root,
         "repo": repo, "from_install": src, "parent": parent, "trust": hits, "tasks": {}}
    m["before"] = snapshot(m)
    save(d, m)
    me = f'"{sys.executable}" "{os.path.abspath(__file__)}"'
    q6 = os.path.join(d, "q6")
    setup = (f'$env:TASKQ_INSTALL="{inst}"; $env:TASKQ_HOST="{a.name}"; & "{exe}" service setup --runtime claude --target "{q6}"' if WIN else
             f'env TASKQ_INSTALL="{inst}" TASKQ_HOST={a.name} "{exe}" service setup --runtime claude --target "{q6}"')
    lines = [f"Fixture {d}", f"Candidate {a.sha} {version} sha256 {m['exe_sha256']} ({b['rustc']}, built {b['finished']})",
             "Cleanup owns only processes whose command line names this fixture folder and Claude jobs inside it.", "",
             f'{me} run {d} --minutes 30 --interrupt-on "#[0-9]+ attempt [0-9]+ access claude"   # terminal 1: Q15 interrupt at the Claude launch, one restart',
             f"{me} tasks {d}                                 # terminal 2, once the fixture heartbeat shows: admission check, held, readback, start",
             "# answer both fixture asks on the board, then in terminal 1:",
             f"{me} run {d} --minutes 45                      # Q1 resume after the answer, same session",
             f"{me} tray {d} --minutes 45                     # terminal 2: Q6, Conversations of the fixture install only",
             f"{setup}   # terminal 3: Q6 setup request; continue it from the fixture app's Conversations",
             f"{me} report {d}", f"{me} cleanup {d}"]
    if WIN:
        task = f"TaskQ-probe-{a.name}"
        lines += ["", "Windows: run/tray need the interactive desktop session; wrap each as a one-time interactive task:",
                  f'schtasks /Create /TN {task} /SC ONCE /ST 23:59 /IT /RL LIMITED /F /TR "\\"{sys.executable}\\" \\"{os.path.abspath(__file__)}\\" run {d} --minutes 45"',
                  f"schtasks /Run /TN {task}", f"schtasks /Delete /TN {task} /F   # after report"]
    open(os.path.join(d, "RUNBOOK.txt"), "w").write("\n".join(lines) + "\n")
    print(d)


GOAL = ("Live probe fixture for taskq#2094 ({q}). Before editing anything, ask one question with two options, `keep` and `remove`. "
        "After the answer, write the chosen word to probe-answer.txt in your worktree without committing and hand in `later` for one day.")


def receipt(m, e, args, tries=3):
    """One task write with a fixed op id; an `unknown` outcome retries the identical call, which only reads back."""
    for i in range(tries):
        _, out = run([m["exe"], *args, "--json"], e, cwd=m["root"])
        try:
            r = json.loads(out.splitlines()[-1])
        except (ValueError, IndexError):
            r = {"outcome": "unknown", "reason": out[-300:]}
        if r.get("outcome") != "unknown":
            return r
        time.sleep(10 * (i + 1))
    return r


def issue(m, e, n):
    _, out = run([m["exe"], "issue", str(n)], e, cwd=m["root"])
    try:
        i = json.loads(out)
    except ValueError:
        sys.exit(f"issue #{n} readback failed: {out[-300:]}")
    return i, {l if isinstance(l, str) else l.get("name") for l in i.get("labels", [])}


def admission(m, rt):
    """Reasons the fixture service would not admit RT now, from its own fresh status; nothing is inferred or granted."""
    try:
        st = json.load(open(os.path.join(m["root"], ".taskq", "status.json")))
        host = json.load(open(os.path.join(m["dir"], "install", "host.json")))
    except (OSError, ValueError) as x:
        return [f"fixture status unknown ({x}); start `run` first"]
    cap = st.get("capacity") or {}
    acc = next((r for r in st.get("access") or [] if r.get("runtime") == rt), None)
    bad = [] if time.time() - (st.get("at") or 0) < 180 else ["fixture status older than 180 s; is `run` live?"]
    bad += [] if st.get("host") == m["name"] else [f"fixture service reports host {st.get('host')}"]
    bad += [] if m["name"] in {h.get("name") for h in st.get("hosts", [])} else ["no board heartbeat for the fixture host yet"]
    bad += [] if host.get("claim") == "labelled" else ["fixture claim is not labelled"]
    bad += [] if "all" in (st.get("focus") or []) else [f"fixture focus {st.get('focus')}"]
    bad += [] if (cap.get("limits") or {}).get(rt, 0) >= 1 and cap.get("gate") == "open" else [f"fixture capacity {rt}: limit {(cap.get('limits') or {}).get(rt)}, gate {cap.get('gate')}"]
    bad += [f"access {rt} refused: {acc.get('refused')}"] if acc and acc.get("refused") else [] if acc else [f"access {rt} unknown"]
    m.setdefault("access", {})[rt] = acc
    return bad


def tasks(a):
    """Per runtime: admission check, held add, label readback, start at the read revision, start readback. Run it
    while `run` is live, since `add --host` needs the fixture's own heartbeat. Rerunning resumes at the first open step."""
    d, m, e = load(a.dir)
    stamp = os.path.basename(d)
    for rt, q in [("claude", "Q1+Q15 claude"), ("codex", "Q1 codex")]:
        t = m["tasks"].setdefault(rt, {})
        if t.get("started"):
            continue
        bad = admission(m, rt)
        save(d, m)
        if bad:
            sys.exit(f"{rt}: not admitted, nothing written: " + "; ".join(bad))
        if not t.get("n"):
            r = receipt(m, e, ["add", f"probe 2094 {q} {m['name']}", "--goal", GOAL.format(q=q), "--acceptance",
                               "Asks before editing; the answer resumes the same session", "--host", m["name"], "--runtime", rt,
                               "--op-id", f"{stamp}-{rt}-add"])
            if r.get("outcome") != "accepted" or r.get("state") != "held" or not r.get("task"):
                save(d, m)
                sys.exit(f"{rt} add: {r}; rerun `tasks` to retry the same op id")
            t.update(n=r["task"], revision=r.get("revision"))
            save(d, m)
        i, labels = issue(m, e, t["n"])
        want = {"q-held", f"host-{m['name']}", f"run-{rt}"}
        if not want <= labels or "taskq" in labels:
            sys.exit(f"#{t['n']} labels {sorted(labels)}, want {sorted(want)} without taskq; not started")
        rev = ["--revision", str(t["revision"])] if t.get("revision") is not None else []
        r = receipt(m, e, ["start", str(t["n"]), "--op-id", f"{stamp}-{rt}-start", *rev])
        if r.get("outcome") != "accepted":
            sys.exit(f"#{t['n']} start: {r}; rerun `tasks` to retry the same op id")
        _, labels = issue(m, e, t["n"])
        if "taskq" not in labels or f"host-{m['name']}" not in labels:
            sys.exit(f"#{t['n']} start readback labels {sorted(labels)}")
        t.update(started=True, revision=r.get("revision"), state=r.get("state"))
        save(d, m)
    print(json.dumps(m["tasks"]))


def kill_tree(p):
    if p.poll() is not None:
        return
    if WIN:
        subprocess.run(["taskkill", "/PID", str(p.pid), "/T", "/F"], capture_output=True)
    else:
        os.killpg(p.pid, signal.SIGTERM)
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
    p.wait()


def supervise(d, name, cmd, env, minutes, interrupt=None):
    """One child at a time; its whole process group dies at the deadline, on exit or on Ctrl-C. Native background
    jobs outlive it on purpose (Q15 reconciliation); `cleanup` stops the fixture's own."""
    deadline = time.time() + min(minutes, 120) * 60
    log = os.path.join(d, "logs", f"{name}.log")
    dispatch = os.path.join(d, "root", ".taskq", "dispatch.log")
    pidfile = os.path.join(d, "logs", f"{name}.pid")
    e = {k: v for k, v in os.environ.items() if k not in STRIP}
    e.update(env)
    starts = 0
    while starts < (2 if interrupt else 1) and time.time() < deadline:
        starts += 1
        seen = os.path.getsize(dispatch) if os.path.exists(dispatch) else 0
        with open(log, "a") as out:
            out.write(f"--- start {starts} {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\n")
            out.flush()
            p = subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, env=e, cwd=os.path.join(d, "root"),
                                 start_new_session=not WIN, creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if WIN else 0)
        open(pidfile, "w").write(f"{os.getpid()}\n")
        hit = False
        try:
            while p.poll() is None and time.time() < deadline and not hit:
                time.sleep(0.5)
                if interrupt and starts == 1 and os.path.exists(dispatch):
                    with open(dispatch, errors="replace") as f:
                        f.seek(seen)
                        hit = re.search(interrupt, f.read()) is not None
        finally:
            kill_tree(p)
        with open(log, "a") as out:
            out.write(f"--- stop {starts} {'interrupted on match' if hit else 'deadline or exit'} exit {p.returncode}\n")
        if not hit:
            break
        time.sleep(5)
    os.remove(pidfile)
    print(f"{name}: {starts} start(s); log {log}")


def run_cmd(a):
    d, m, e = load(a.dir)
    bad = reconcile(m["parent"], m["repo"], own=d)
    if bad:
        sys.exit("earlier probe not reconciled, service not launched:\n" + "\n".join(bad))
    supervise(d, "service", [m["exe"], "service", "run"], e, a.minutes, a.interrupt_on)


def tray(a):
    d, m, e = load(a.dir)
    t = os.path.join(d, "tmp")
    supervise(d, "tray", [m["exe"], "tray"], dict(e, TMPDIR=t, TMP=t, TEMP=t), a.minutes)


def jobs(m):
    """Claude background jobs whose folder is inside the fixture (`claude agents`, read-only)."""
    dirs = [m["root"], os.path.join(m["dir"], "q6")]
    for base in [os.path.join(m["root"], ".worktrees"), os.path.join(m["dir"], "install", "setup")]:
        dirs += [os.path.join(base, x) for x in (os.listdir(base) if os.path.isdir(base) else []) if os.path.isdir(os.path.join(base, x))]
    rows = {}
    for c in dirs:
        rc, out = run(["claude", "agents", "--json", "--all", "--cwd", c], timeout=30)
        try:
            got = json.loads(out) if rc == 0 else []
        except ValueError:
            got = []
        for r in got if isinstance(got, list) else got.get("agents", []):
            if key(str(r.get("cwd") or c)).startswith(key(m["dir"])):
                rows[r.get("id")] = r
    return list(rows.values())


def receipts(m):
    out = {}
    q = os.path.join(m["root"], ".taskq")
    for f in sorted(os.listdir(q)) if os.path.isdir(q) else []:
        if not re.match(r"[TR]\d+-\d+\.log$", f):
            continue
        execs, bridge = [], set()
        for line in open(os.path.join(q, f), errors="replace"):
            try:
                v = json.loads(line)
            except ValueError:
                continue
            if v.get("type") == "taskq.input":
                execs.append({"execution": v.get("execution"), "runtime": v.get("runtime"), **{k: (v.get("native") or {}).get(k) for k in ("session", "job")}})
            elif v.get("type") == "bridge-session":
                bridge.add(v.get("sessionId"))
        sessions = {x["session"] for x in execs if x["session"]}
        out[f] = {"executions": execs, "same session": len(sessions) == 1 and len(execs) > 1, "remote control sessions": sorted(bridge)}
    setup = os.path.join(m["dir"], "install", "setup")
    for f in sorted(os.listdir(setup)) if os.path.isdir(setup) else []:
        if f.endswith(".json"):
            try:
                r = json.load(open(os.path.join(setup, f)))
            except (OSError, ValueError):
                continue
            out[f"setup/{f}"] = {k: r.get(k) for k in ("id", "runtime", "state", "job", "session", "reason")} | {"remote control": bool(r.get("url"))}
    return out


def report(a):
    d, m, e = load(a.dir)
    after = snapshot(m)
    dispatch = os.path.join(m["root"], ".taskq", "dispatch.log")
    try:
        agents = json.load(open(os.path.join(m["root"], ".taskq", "status.json"))).get("agents")
    except (OSError, ValueError):
        agents = "unknown"
    r = {"candidate": m["sha"], "exe_sha256": m["exe_sha256"], "exe unchanged": sha256(m["exe"]) == m["exe_sha256"], "version": m["version"],
         "os": platform.platform(), "claude": run(["claude", "--version"])[1], "codex": run(["codex", "--version"])[1], "tasks": m["tasks"],
         "inbox": {rt: run([m["exe"], "inbox", str(t["n"]), "--json"], e, cwd=m["root"])[1][-4000:] for rt, t in m["tasks"].items() if t.get("n")},
         "access": m.get("access"),
         "receipts": receipts(m), "status agents": agents, "fixture claude jobs": jobs(m),
         "unchanged": {k: ("unchanged" if after.get(k) == v else "CHANGED") for k, v in m["before"].items()},
         "dispatch": [l.rstrip() for l in open(dispatch, errors="replace")][-200:] if os.path.exists(dispatch) else []}
    json.dump(r, open(os.path.join(d, "report.json"), "w"), indent=1)
    print(json.dumps({k: r[k] for k in ("candidate", "exe_sha256", "exe unchanged", "version", "claude", "codex", "tasks", "unchanged")}, indent=1))
    for f, v in r["receipts"].items():
        print(f, json.dumps(v))


def procs(m):
    """Processes, other than this one, whose command line names the fixture folder (codex exec, app server, children)."""
    if WIN:
        _, out = run(["powershell", "-NoProfile", "-Command", "Get-CimInstance Win32_Process | % { \"$($_.ProcessId) $($_.CommandLine)\" }"])
    else:
        _, out = run(["ps", "-axww", "-o", "pid=,command="])
    # ponytail: command-line match only; a process that names nothing of the fixture is missed, add a cwd scan if one shows up
    want = norm(m["dir"])
    rows = [l.strip().split(" ", 1) for l in out.splitlines()]
    return [(int(r[0]), r[1]) for r in rows if len(r) == 2 and r[0].isdigit() and int(r[0]) != os.getpid() and want in norm(r[1])]


def cleanup(a):
    """In order, stopping at the first step that cannot be read back, with the fixture kept: no live supervisor;
    a fresh report; each fixture task closed by op id and read back closed; every fixture Claude job stopped; every
    other fixture process ended; receipts archived next to the fixture; only then the fixture deleted."""
    d, m, e = load(a.dir)
    for name in ["service", "tray"]:
        f = os.path.join(d, "logs", f"{name}.pid")
        if os.path.exists(f):
            sys.exit(f"`{name}` supervisor may still run (pid {open(f).read().strip()}); stop it with Ctrl-C first")
    report(a)
    for rt, t in m["tasks"].items():
        if not t.get("n") or t.get("closed"):
            continue
        r = receipt(m, e, ["close", str(t["n"]), "--text", "Probe fixture for taskq#2094; receipts kept by the PM", "--op-id", f"{os.path.basename(d)}-{rt}-close"])
        i, _ = issue(m, e, t["n"])
        if str(i.get("state")).lower() not in ("closed", "close"):
            sys.exit(f"#{t['n']} close: {r.get('outcome')} {r.get('reason')}; issue still {i.get('state')}; fixture kept")
        t["closed"] = True
        save(d, m)
    for _ in range(3):
        live = [r for r in jobs(m) if str(r.get("state")) in LIVE]
        for r in live:
            print("stop", r.get("id"), run(["claude", "stop", str(r.get("id"))], timeout=30)[1])
        if not live:
            break
        time.sleep(5)
    if [r for r in jobs(m) if str(r.get("state")) in LIVE]:
        sys.exit("fixture Claude jobs still live; fixture kept")
    for p, cmd in procs(m):
        print("end", p, cmd[:120])
        if WIN:
            subprocess.run(["taskkill", "/PID", str(p), "/T", "/F"], capture_output=True)
        else:
            try:
                os.kill(p, signal.SIGTERM)
            except OSError:
                pass
    time.sleep(5)
    if procs(m):
        sys.exit(f"fixture processes still live: {[p for p, _ in procs(m)]}; fixture kept")
    keep = [os.path.join(d, x) for x in ["probe.json", "report.json", "RUNBOOK.txt", "logs", os.path.join("install", "setup")]]
    q = os.path.join(m["root"], ".taskq")
    keep += [os.path.join(q, f) for f in (os.listdir(q) if os.path.isdir(q) else []) if f.endswith(".log") or f in ("status.json", "events")]
    arc = d + "-receipts.tar.gz"
    with tarfile.open(arc, "w:gz") as tar:
        for p in keep:
            if os.path.exists(p):
                tar.add(p, arcname=os.path.relpath(p, d))
    os.chmod(arc, 0o600)
    with tarfile.open(arc) as tar:
        if "report.json" not in tar.getnames():
            sys.exit(f"{arc} lacks report.json; fixture kept")
    shutil.rmtree(d)
    print(f"receipts {arc}; removed {d}")


p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
s = p.add_subparsers(dest="cmd", required=True)
for name in ["identity", "prepare"]:
    q = s.add_parser(name)
    q.add_argument("--repo", required=True)
    q.add_argument("--from-install", required=True)
    q.add_argument("--parent", required=True)
    if name == "prepare":
        q.add_argument("--build", required=True)
        q.add_argument("--name", required=True)
q = s.add_parser("build")
q.add_argument("--sha", required=True)
q.add_argument("--repo", required=True)
q.add_argument("--out", required=True)
q.add_argument("--minutes", type=int, default=90)
for name in ["tasks", "report", "cleanup"]:
    s.add_parser(name).add_argument("dir")
for name in ["run", "tray"]:
    q = s.add_parser(name)
    q.add_argument("dir")
    q.add_argument("--minutes", type=int, default=30)
    if name == "run":
        q.add_argument("--interrupt-on")
a = p.parse_args()
{"identity": identity, "build": build, "prepare": prepare, "tasks": tasks, "run": run_cmd, "tray": tray, "report": report, "cleanup": cleanup}[a.cmd](a)
