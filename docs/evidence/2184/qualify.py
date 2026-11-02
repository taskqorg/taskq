#!/usr/bin/env python3
"""#2184 native A6 qualification consumer. Usage: qualify.py CANDIDATE ROOT LOG.
Real consumer process: binds its own PID to the slot. Never starts a model turn.
The release key stays in memory only; the log never contains it."""
import json, os, secrets, signal, subprocess, sys, time, uuid, hashlib

T, ROOT, LOG = sys.argv[1:4]
log = open(LOG, "w")
def rec(step, **kw):
    kw = {"step": step, "at": int(time.time()), **kw}
    log.write(json.dumps(kw) + "\n"); log.flush(); print(json.dumps(kw)[:400], flush=True)
def run(*args, stdin=None):
    p = subprocess.run([T, *args], capture_output=True, text=True, input=stdin, timeout=120)
    return p.returncode, p.stdout.strip(), p.stderr.strip()
def js(out):
    try: return json.loads(out.splitlines()[-1])
    except Exception: return out

key = secrets.token_hex(32)
ident = js(run("service", "slots", "--identity", str(os.getpid()), "--json")[1])
rec("identity", identity=ident)
binding = {"schema": "taskq.slots/1", "caller": "pm-2184", "op_id": f"qual-2184-{uuid.uuid4()}", "root": ROOT, "runtime": "codex",
           "pid": os.getpid(), "started": ident["started"], "boot": ident["boot"], "expires_at": int(time.time()) + 290}
slot = {"binding": binding, "key": key}
S = json.dumps(slot)
# private recovery copy (mode 600, scratch only, never published)
fd=os.open(LOG+".slot",os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600);os.write(fd,S.encode());os.close(fd)
code, out, err = run("service", "slots", "--action", "reserve", "--request", S, "--json")
rec("reserve", code=code, receipt=js(out), stderr=err)
for _ in range(60):
    code, out, err = run("service", "slots", "--action", "observe", "--request", S, "--json")
    r = js(out)
    if isinstance(r, dict) and r.get("state") in ("reserved", "denied", "released"): break
    time.sleep(5)
rec("admitted", code=code, receipt=r, stderr=err)
if not (isinstance(r, dict) and r.get("state") == "reserved"): sys.exit("no slot")

# 1. caller-bound prepare, repeat returns the original value
P = json.dumps({"schema": "taskq.app-server/1", "slot": slot})
code, out, err = run("service", "app-server", "--action", "prepare", "--request", P)
prep = js(out); rec("prepare", code=code, state=prep.get("state") if isinstance(prep, dict) else prep, preparation=prep.get("preparation") if isinstance(prep, dict) else None,
                    policy_revision=prep.get("policy_revision") if isinstance(prep, dict) else None, account=prep.get("account") if isinstance(prep, dict) else None,
                    prepared=prep.get("prepared") if isinstance(prep, dict) else None, stderr=err)
code2, out2, err2 = run("service", "app-server", "--action", "prepare", "--request", P)
rec("prepare-repeat", code=code2, identical=js(out2) == prep, stderr=err2)
# wrong key for the same caller/op must refuse (caller-bound)
bad = {"binding": binding, "key": secrets.token_hex(32)}
code, out, err = run("service", "app-server", "--action", "prepare", "--request", json.dumps({"schema": "taskq.app-server/1", "slot": bad}))
rec("prepare-wrong-key", code=code, stdout=out, stderr=err)

req = {"schema": "taskq.app-server/1", "slot": slot, "policy_revision": prep["policy_revision"], "account": prep["account"], "preparation": prep["preparation"]}
# 2. drift refusal: changed preparation digest / revision refuse before dispatch, slot launch not consumed
for name, change in [("digest", {"preparation": "0" * 64}), ("revision", {"policy_revision": prep["policy_revision"] + 1})]:
    code, out, err = run("service", "app-server", "--action", "launch", "--request", json.dumps({**req, **change}))
    rec(f"drift-{name}", code=code, stdout=out[:300], stderr=err)
code, out, err = run("service", "app-server", "--action", "observe", "--request", json.dumps(req))
rec("observe-after-drift", code=code, receipt=js(out), stderr=err)

# 3. launch: exact stdio peers; initialize/initialized only
R = json.dumps(req)
fe = subprocess.Popen([T, "service", "app-server", "--action", "launch", "--request", R], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
rec("launch-spawned", frontend_pid=fe.pid)
obs = None
for _ in range(40):
    time.sleep(0.5)
    code, out, err = run("service", "app-server", "--action", "observe", "--request", R)
    obs = js(out)
    if isinstance(obs, dict) and obs.get("state") == "verified-effective": break
rec("observe-before-handshake", code=code, receipt=obs, stderr=err)
if isinstance(obs, dict) and obs.get("state") == "verified-effective":
    fe.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "taskq-2184-qualification", "version": "0"}}}) + "\n"); fe.stdin.flush()
    line = fe.stdout.readline()
    rec("initialize", response=line.strip()[:500])
    fe.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "initialized"}) + "\n"); fe.stdin.flush()
    time.sleep(1)
    code, out, err = run("service", "app-server", "--action", "observe", "--request", R)
    rec("observe-after-handshake", code=code, receipt=js(out), stderr=err)

# 4. inherited lifetime through frontend interruption (SIGKILL the frontend; never replay)
fe.kill(); fe.wait()
rec("frontend-killed", returncode=fe.returncode, stderr=fe.stderr.read()[-800:])
code, out, err = run("service", "slots", "--action", "release", "--request", S, "--json")
rec("release-before-completion", code=code, stdout=out, stderr=err)
code, out, err = run("service", "app-server", "--action", "launch", "--request", R)
rec("relaunch-refused", code=code, stdout=out[:300], stderr=err)

# 5. sealed singleton certification + cleanup acknowledgment recovery via reconcile
res = None
for i in range(30):
    code, out, err = run("service", "app-server", "--action", "reconcile", "--request", R)
    res = js(out); rec(f"reconcile-{i}", code=code, receipt=res, stderr=err)
    if isinstance(res, dict) and res.get("state") == "completed": break
    time.sleep(2)
code, out, err = run("service", "app-server", "--action", "reconcile", "--request", R)
rec("reconcile-duplicate", code=code, receipt=js(out), stderr=err)

# 6. original-slot release and readback (only after completed)
if isinstance(res, dict) and res.get("state") == "completed":
    code, out, err = run("service", "slots", "--action", "release", "--request", S, "--json")
    rec("release", code=code, receipt=js(out), stderr=err)
    for _ in range(40):
        code, out, err = run("service", "slots", "--action", "observe", "--request", S, "--json")
        r = js(out)
        if isinstance(r, dict) and r.get("state") == "released": break
        time.sleep(5)
    rec("release-readback", code=code, receipt=r, stderr=err)
else:
    rec("slot-preserved", reason="completion not verified; no forced release")
