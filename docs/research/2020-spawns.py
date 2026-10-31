"""Classify every `spawn` entry of long issues by what followed it (#2020).

Usage: spawns.py FILE.json..  (each file: `gh api --paginate --slurp repos/<o>/<r>/issues/<n>/comments?per_page=100`,
       or a GitLab notes list saved as {"gitlab": [...notes...]})
Prints one summary row per issue, then the unaccepted spawns with their class.

Classes of a spawn S (entry id S):
  accepted   a later `taskq: accepted S attempt S` exists
  ran        no `accepted S`, but a later entry carries `attempt S` (the child ran; acceptance never written)
  no-start   `released S no-start attempt S` (fold accepted the claim; delivery confirmed it never started)
  lost       no later entry names S: fold did not take the claim, or the claim was never delivered and never released
"""
import json, re, sys
from collections import Counter

HEAD = re.compile(r"taskq: (\S+)(.*)")


def notes(path):
    data = json.load(open(path, encoding="utf-8"))
    if isinstance(data, dict):  # GitLab notes
        return [(n["id"], n["created_at"], n["body"], n["updated_at"] != n["created_at"]) for n in data["gitlab"] if not n["system"]]
    flat = [c for page in data for c in page] if data and isinstance(data[0], list) else data
    return [(c["id"], c["created_at"], c["body"], c["updated_at"] != c["created_at"]) for c in flat]


def entries(path):
    out = []
    for cid, at, body, edited in sorted(notes(path)):
        m = HEAD.match(body.split("\n", 1)[0].strip())
        if not m or edited:
            continue
        words = m.group(2).split()
        attempt = int(words[-1]) if len(words) >= 2 and words[-2] == "attempt" and words[-1].isdigit() else None
        out.append({"id": cid, "at": at, "kind": m.group(1), "args": words, "attempt": attempt, "body": body})
    return out


def classify(log):
    rows = []
    for i, e in enumerate(log):
        if e["kind"] != "spawn":
            continue
        s = e["id"]
        later = log[i + 1:]
        named = [x for x in later if x["attempt"] == s]
        if any(x["kind"] == "accepted" and x["args"][:1] == [str(s)] for x in named):
            c = "accepted"
        elif any(x["kind"] == "released" and x["args"][:2] == [str(s), "no-start"] for x in named):
            c = "no-start"
        elif named:
            c = "ran"
        else:
            c = "lost"
        prev = log[i - 1] if i else None
        nxt = later[0] if later else None
        rows.append({"id": s, "at": e["at"], "role": e["args"][0], "rt": e["args"][1].split(":")[0], "host": e["args"][2] if len(e["args"]) > 2 else "?",
                     "class": c, "prev": prev and prev["kind"], "next": nxt and nxt["kind"], "next_at": nxt and nxt["at"],
                     "kinds": Counter(x["kind"] for x in named)})
    return rows


def main(paths):
    total = Counter()
    for p in paths:
        log = entries(p)
        rows = classify(log)
        acc = sum(1 for x in log if x["kind"] == "accepted")
        c = Counter(r["class"] for r in rows)
        total.update(c)
        first_acc = next((x["at"] for x in log if x["kind"] == "accepted"), "-")
        print(f"{p.rsplit('/', 1)[-1]}: comments {len(notes(p))} spawn {len(rows)} accepted-entries {acc} classes {dict(c)} first-accepted {first_acc}")
        for r in rows:
            if r["class"] != "accepted":
                print(f"   {r['id']} {r['at']} {r['role']} {r['rt']} {r['host']} {r['class']} prev={r['prev']} next={r['next']}@{r['next_at']} named={dict(r['kinds'])}")
    print("total", dict(total))


if __name__ == "__main__":
    main(sys.argv[1:])
