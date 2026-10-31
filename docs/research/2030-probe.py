#!/usr/bin/env python3
"""GitLab issue-note option probe for taskq#2030 (A1-A2 of #2016 A3).

Creates one scratch issue without labels, tries note collapse, thread resolve
and internal-note options through REST and GraphQL, posts and edits a note,
records the notes list before and after, then closes the issue. Prints JSON
readbacks to stdout. Token: <install>/gitlab-token (never printed).

Usage: python3 -I 2030-probe.py <host> <project path> <token file>
"""
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

HOST, PROJECT, TOKEN_FILE = sys.argv[1:4]
TOKEN = open(TOKEN_FILE).read().strip()
API = f"https://{HOST}/api/v4"
PID = urllib.parse.quote(PROJECT, safe="")
log = []


def call(method, path, data=None, graphql=False):
    url = f"https://{HOST}/api/graphql" if graphql else API + path
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, data=body, method=method)
    req.add_header("Authorization", f"Bearer {TOKEN}")
    if body is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req) as r:
            status, raw = r.status, r.read()
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read()
    try:
        out = json.loads(raw) if raw else None
    except ValueError:
        out = raw.decode(errors="replace")[:300]
    return status, out


def step(name, method, path, data=None, keep=None, graphql=False):
    status, out = call(method, path, data, graphql)
    shown = out
    if keep and isinstance(out, dict):
        shown = {k: out.get(k) for k in keep}
    log.append({"step": name, "request": f"{method} {'graphql' if graphql else path}",
                "payload": data, "http": status, "response": shown})
    return status, out


NOTE_KEYS = ["id", "body", "created_at", "updated_at", "system", "internal",
             "confidential", "resolvable", "resolved", "resolved_by", "type"]


def notes(label, iid):
    rows = {}
    for order in ("default", "created_at asc"):
        q = "" if order == "default" else "?order_by=created_at&sort=asc"
        status, out = call("GET", f"/projects/{PID}/issues/{iid}/notes{q}")
        rows[order] = [{k: n.get(k) for k in NOTE_KEYS} for n in out] if status == 200 else out
    log.append({"step": label, "request": f"GET /projects/:id/issues/{iid}/notes (default; order_by=created_at&sort=asc)",
                "http": status, "response": rows})


step("version", "GET", "/version")
_, issue = step("create scratch issue", "POST", f"/projects/{PID}/issues",
                {"title": "scratch: note option probe (taskq#2030)",
                 "description": "Scratch issue for taskq#2030; closed after the probe. Not a task."},
                keep=["iid", "labels", "state", "created_at"])
iid = issue["iid"]
N = f"/projects/{PID}/issues/{iid}/notes"
D = f"/projects/{PID}/issues/{iid}/discussions"

# A2 part 1: a plain note, then the list before any edit.
_, plain = step("post plain note", "POST", N, {"body": "probe note: original text"}, keep=NOTE_KEYS)
notes("notes before edit", iid)

# A1: internal note (current and legacy parameter names).
_, internal = step("post internal note (internal=true)", "POST", N,
                   {"body": "probe note: internal", "internal": True}, keep=NOTE_KEYS)
step("post internal note (confidential=true, deprecated)", "POST", N,
     {"body": "probe note: confidential", "confidential": True}, keep=NOTE_KEYS)
step("flip plain note to internal via PUT", "PUT", f"{N}/{plain['id']}",
     {"internal": True}, keep=NOTE_KEYS)

# A1: thread resolve.
_, disc = step("start discussion thread", "POST", D, {"body": "probe thread"},
               keep=["id", "individual_note", "notes"])
did = disc["id"]
step("resolve discussion (PUT discussions/:id resolved=true)", "PUT", f"{D}/{did}",
     {"resolved": True}, keep=["id", "individual_note", "notes"])
step("resolve thread note (PUT notes/:id resolved=true)", "PUT", f"{N}/{disc['notes'][0]['id']}",
     {"resolved": True}, keep=NOTE_KEYS)

# A1: collapse. REST has no collapse field; unknown params are tried and read back.
for param in ("collapsed", "hidden", "minimized"):
    step(f"collapse via PUT note {param}=true", "PUT", f"{N}/{plain['id']}",
         {param: True}, keep=NOTE_KEYS)

# A1: GraphQL schema: which note fields and mutations exist.
step("graphql Note fields", "POST", None, {"query": "{ __type(name: \"Note\") { fields { name } } }"}, graphql=True)
step("graphql UpdateNoteInput fields", "POST", None,
     {"query": "{ __type(name: \"UpdateNoteInput\") { inputFields { name } } }"}, graphql=True)
step("graphql note mutations", "POST", None,
     {"query": "{ __schema { mutationType { fields { name } } } }"}, graphql=True)
step("graphql discussionToggleResolve on issue thread", "POST", None,
     {"query": "mutation($id: DiscussionID!) { discussionToggleResolve(input: {id: $id, resolve: true}) { errors discussion { resolved } } }",
      "variables": {"id": f"gid://gitlab/Discussion/{did}"}}, graphql=True)

# A2 part 2: edit the plain note, then the list after.
step("edit plain note body", "PUT", f"{N}/{plain['id']}",
     {"body": "probe note: edited text"}, keep=NOTE_KEYS)
notes("notes after edit", iid)

step("close scratch issue", "PUT", f"/projects/{PID}/issues/{iid}",
     {"state_event": "close"}, keep=["iid", "state", "closed_at", "labels"])

json.dump(log, sys.stdout, indent=1)
print()
