"""Measure issue pages with the docs/research/1980.md method (#1984).

Usage: GITLAB_TOKEN=... measure.py fetch  -> writes issues.json (GitLab csgo/cs + GitHub calibration via gh)
       measure.py table  -> prints the per-issue table and totals
"""
import json, os, re, subprocess, sys, urllib.parse

GL = "https://gitlab.ufobe.com/api/v4/projects/"
SAMPLE = {
    "alex/csgo": [323, 336, 255, 434, 424, 393, 432, 437, 356],
    "gradus/cyprusbz": [528, 541, 469, 533, 534, 535, 537, 539],
}
CALIB = [1721, 1929, 1758, 1132, 1521, 1702, 1053]  # taskq#, read-now times in 1980.md
SERVICE = {"legacy-service", "hold", "start", "spawn", "accepted", "released", "executing", "enqueued", "gate",
           "rebased", "usage-limit", "approve", "report", "result-ref", "report-ref"}
OUTCOME = {"result", "ask", "publish", "close", "reject"}
# Legacy v3 entries ("**take** · codex:<id>") still fill older GitLab issues; their service-only kinds.
LEGACY_SERVICE = {"take", "beat", "shown", "spawn", "run", "adopt", "runtime", "reserve", "launch",
                  "ready", "waiting", "deps", "edit"}
STRICT = os.environ.get("STRICT") == "1"  # 1: only "taskq:" headers count, as 1980.md read GitHub
FILE_RE = re.compile(r"!\[[^\]]*\]\([^)]+\)|\]\((?:[^)]*/uploads/[^)]+|https://github\.com/user-attachments/[^)]+)\)")


def gl(path):
    tok = os.environ["GITLAB_TOKEN"]  # a read token for gitlab.ufobe.com
    out, page = [], 1
    while True:
        url = f"{path}{'&' if '?' in path else '?'}per_page=100&page={page}"
        # curl: the python.org build has no CA bundle; token goes through stdin config, not argv
        data = json.loads(subprocess.run(["curl", "-sf", "-K", "-", url], check=True, capture_output=True,
                                         input=f'header = "PRIVATE-TOKEN: {tok}"', text=True).stdout)
        if not isinstance(data, list):
            return data
        out += data
        if len(data) < 100:
            return out
        page += 1


def fetch():
    issues = []
    for repo, iids in SAMPLE.items():
        base = GL + urllib.parse.quote(repo, safe="")
        for iid in iids:
            i = gl(f"{base}/issues/{iid}")
            notes = gl(f"{base}/issues/{iid}/notes?sort=asc&order_by=created_at")
            issues.append({"ref": f"{repo.split('/')[1]}#{iid}", "url": i["web_url"], "state": i["state"],
                           "labels": i["labels"], "body": i["description"] or "", "title": i["title"],
                           "notes": [{"body": n["body"], "system": n["system"],
                                      "edited": n["updated_at"] != n["created_at"]} for n in notes]})
    for n in CALIB:
        i = json.loads(subprocess.check_output(["gh", "api", f"repos/taskqorg/taskq/issues/{n}"]))
        cs = json.loads(subprocess.check_output(["gh", "api", "--paginate", "--slurp",
                                                 f"repos/taskqorg/taskq/issues/{n}/comments?per_page=100"]))
        cs = [c for p in cs for c in p]
        issues.append({"ref": f"taskq#{n}", "url": i["html_url"], "state": i["state"],
                       "labels": [l["name"] for l in i["labels"]], "body": i["body"] or "", "title": i["title"],
                       "notes": [{"body": c["body"], "system": False,
                                  "edited": c["updated_at"] != c["created_at"]} for c in cs]})
    json.dump(issues, open("issues.json", "w"), ensure_ascii=False, indent=1)


def kind(body):
    m = re.match(r"taskq: (\S+)", body)
    if m:
        return m.group(1)
    m = None if STRICT else re.match(r"\*\*([a-z-]+)\*\* · \S+:", body)
    if m:
        return "legacy-service" if m.group(1) in LEGACY_SERVICE else m.group(1)
    return None


def classify(body):
    """Yield (class, line) per nonblank line with the 1980 rules."""
    k = kind(body)
    for idx, line in enumerate(body.splitlines()):
        if not line.strip():
            continue
        s = line.strip()
        if k is None:
            yield "human", line
        elif idx == 0 or s.startswith(("op-id:", "revision:", "<!-- taskq")) or k in SERVICE:
            yield "machine", line
        elif s.startswith("To answer:"):
            yield "boiler", line
        elif s.startswith(("Checks:", "delivery", "merged", "enqueued")):
            yield "both", line
        else:
            yield "human", line


def goal(body):
    m = re.search(r"## Goal\s*\n(.*?)(?:\n## |\Z)", body, re.S)
    return (m.group(1) if m else body[:190]).strip()


def human_chars(body):
    return sum(len(l) for c, l in classify(body) if c in ("human", "both"))


def measure(i):
    notes = [n for n in i["notes"] if not n["system"]]
    bodies = [n["body"] for n in notes]
    lines = [cl for b in bodies for cl in classify(b)]
    machine = sum(1 for c, _ in lines if c == "machine")
    human = sum(1 for c, _ in lines if c in ("human", "both"))
    tech_chars = sum(len(l) for c, l in lines if c != "human")
    chars = len(i["body"]) + sum(len(b) for b in bodies)
    service = sum(1 for b in bodies if kind(b) in SERVICE)
    files_body = len(FILE_RE.findall(i["body"]))
    files_notes = [len(FILE_RE.findall(b)) for b in bodies]
    out_idx = max((x for x, b in enumerate(bodies) if kind(b) in OUTCOME), default=None)
    heartbeat = "taskq-host" in i["labels"]

    # Today (1980 model): 2 s labels; skim up from the bottom at 0.6 s per comment to the latest
    # outcome; read its human+both lines at 25 chars/s; files outside it cost one skim of the thread.
    t = 2.0
    if heartbeat:
        t += len(i["body"][:190]) / 25
    elif out_idx is None:
        t += len(goal(i["body"])) / 25
    else:
        t += 0.6 * (len(bodies) - out_idx) + human_chars(bodies[out_idx]) / 25
    files = files_body + sum(files_notes)
    if files and (out_idx is None or files_notes[out_idx] < files):
        t += 0.6 * len(bodies)

    # Option B: labels plus the summary comment under the body: state line, latest outcome
    # (Changed + Open, or ask Problem + options), decisions line, one 40-char label per file.
    if heartbeat:
        b = t
    else:
        summ = 40  # state line, e.g. "Closed · merged !123 · 2026-10-30 12:00Z"
        if out_idx is None:
            summ += len(goal(i["body"]))
        else:
            ob = bodies[out_idx].splitlines()
            if kind(bodies[out_idx]) == "ask":
                keep = [l for l in ob if l.startswith(("Problem:",)) or re.match(r"\d\. ", l)]
            else:
                keep = [l for l in ob if l.startswith(("Changed:", "Open:"))]
            if not keep:  # legacy outcome without Changed/Open: its first two human lines, 160 chars each
                keep = [l[:160] for c, l in classify(bodies[out_idx]) if c == "human"][:2]
            summ += sum(len(l) for l in keep)
        decisions = sum(1 for x in bodies if kind(x) in ("answer", "steer"))
        summ += 60 if decisions else 0
        summ += 40 * files
        b = 2.0 + summ / 25

    where = []
    alltext = i["body"] + "\n".join(bodies)
    if any(kind(x) == "result" for x in bodies) or re.search(r"/-/jobs/artifacts|/-/packages/", alltext):
        where.append("result")
    if re.search(r"merge_requests/\d+|(?<![\w/])!\d+|/pull/\d+", "\n".join(bodies)):
        where.append("MR")
    if re.search(r"docs/research/|Report:", "\n".join(bodies)):
        where.append("report")
    where = ", ".join(where) or ("body edited in place" if heartbeat else "none")
    if files_body:
        where += f"; {files_body} file(s) in body"
    if sum(files_notes):
        where += f"; {sum(files_notes)} file(s) in comments"
    return dict(ref=i["ref"], url=i["url"], state="heartbeat" if heartbeat else i["state"],
                comments=len(bodies), system=len(i["notes"]) - len(bodies), chars=chars,
                machine=machine, human=human, tech=tech_chars, service=service,
                edited=sum(1 for n in notes if n["edited"]), now=round(t, 1), b=round(b, 1), where=where,
                no_header=sum(1 for x in bodies if kind(x) is None))


def table():
    rows = [measure(i) for i in json.load(open("issues.json"))]
    print("| Issue | State | Comments | System notes | Chars | Machine lines | Human lines | Read now (s) | Read B (s) | Result lives in |")
    print("|---|---|---:|---:|---:|---:|---:|---:|---:|---|")
    for r in rows:
        print(f"| [{r['ref']}]({r['url']}) | {r['state']} | {r['comments']} | {r['system']} | {r['chars']:,} | "
              f"{r['machine']} | {r['human']} | {r['now']} | {r['b']} | {r['where']} |")
    print()
    for board in ("csgo", "cyprusbz", "taskq"):
        g = [r for r in rows if r["ref"].startswith(board + "#")]
        tasks = [r for r in g if r["state"] != "heartbeat"]
        now = sorted(r["now"] for r in tasks)
        bb = sorted(r["b"] for r in tasks)
        med = lambda v: (v[len(v) // 2] if len(v) % 2 else (v[len(v) // 2 - 1] + v[len(v) // 2]) / 2)
        print(board, dict(issues=len(g), comments=sum(r["comments"] for r in g), system=sum(r["system"] for r in g),
                          chars=sum(r["chars"] for r in g), service=sum(r["service"] for r in g),
                          tech=sum(r["tech"] for r in g), machine=sum(r["machine"] for r in g),
                          human=sum(r["human"] for r in g), edited=sum(r["edited"] for r in g),
                          no_header=sum(r["no_header"] for r in g),
                          now_med=med(now), now_max=max(now), now_le30=sum(v <= 30 for v in now),
                          b_med=med(bb), b_max=max(bb), b_le30=sum(v <= 30 for v in bb), tasks=len(tasks)))


if __name__ == "__main__":
    {"fetch": fetch, "table": table}[sys.argv[1]]()
