# Configuration

Project and host configuration keys, recipes and cutover procedures.

## Contract detail

Linked from `taskq.md` [§ 9](../../taskq.md#9-config-and-cutover); this text binds like the contract (R8, R13).

### Keys

`taskq.json` beside the project root:

- `board` `github|gitlab`; `repo` `owner/name`; `host` the board's web host (default `github.com`, `gitlab.com`).
- `label_schema` `canonical|legacy` (absent: legacy if the board has `priority-1|2` or `q-waiting` objects, else canonical). Both names work in priority and focus/status filters; `p0` has no legacy name; logs keep `waiting`. Canonical writes on existing boards need all-host compatible release/idle approval; legacy objects and closed issues stay, only open issues are reassigned.
- `publish` `direct|pr`, `delivery` (§ 7); `workspace` `external` keeps host-owned worktrees.
- Host-owned `<install>/host.json`: `hosts` (hostname: name; TASKQ_HOST overrides), `has` (name: capabilities), runtime `limits`, `admission` {"max": n}, `load_gate`, `codex` args, `permission_mode` (R9), `channel` (§ 8), `claim` `any|labelled` (default `any`, § 4; `add` defaults `--host` to this host), `default_runtime` `claude|codex` (default `claude`; [detail](host-service.md#default-runtime)), `projects` {root: {limits}}, lower-only ([remote](shared-resource-controls.md#project-overlay)). No capacity_hosts/mode. Keep Auto/D03 defaults; migration enables no gate, changes no permissions.
- Design, UI, visual and image work goes to codex with `add --runtime codex` ([specq](https://github.com/taskqorg/specq/blob/main/engineering/agent-workflow.md#runtime-preference)); `add --prefer` is refused, hinting `--runtime`. Opt-in `codex-app` (macOS, Windows): only on hosts whose `has` lists it, only for `run-codex-app` tasks (`add --runtime codex-app`); reviewer chain checks keep app-worker cases on macOS and Windows and none elsewhere. A screen task adds `needs-screen` (`has` `screen`). macOS evidence tasks: [detail](../runtime-qualification.md#macos-evidence-tasks).
- Project taskq.json keeps board/repo/board host, publish/delivery, workspace, labels, recipes, incident_reports, pages, assignee and other project keys; `limits` per § 3. No host identity, capability, model/permission or admission copies. Token lookup and worker-policy grants (§ 8 Access) stay unchanged; host flags never grant access.
- `recipes` `{name: {"version": V, "tasks": [{"title", "goal", "acceptance": [..], "scope", "deps": [earlier 1-based index..], "gate", "type", "runtime", "priority", "host"}]}}` (§ 7).

### Six-copy migration

- [Six-copy migration](host-service.md#six-copy-configuration-migration) compares inputs before writes; conflicts/unknowns hold launches. Keep originals, unknown keys and ownership; replacement resumes idempotently.

### Publication cutover and contract cutover procedure

Publication cutover: keep results, stop old services/children, set public GitHub queues/required `merge_group` CI, upgrade hosts, resume.

An agent reads entries from the board's version to the new one, plans each open issue and asks the owner about issues the new format cannot hold. After the owner's OK, with old services and children stopped, it applies the plan with `taskq` commands and issue edits. Closed issues are never touched. New services start: Mac, then Windows. Repair: the same procedure for one project, warning when `status` shows another host's fresh heartbeat.
