# Configuration

Project and host configuration keys, recipes and cutover procedures.

## Contract detail

Linked from `taskq.md` [§ 9](../../taskq.md#9-config-and-cutover); this text binds like the contract (R8, R13).

### Keys

`taskq.json` beside the project root:

- `board` `github|gitlab`; `repo` `owner/name`; `host` the board's web host (default `github.com`, `gitlab.com`).
- `label_schema` `canonical|legacy` (absent: legacy if the board has `priority-1|2` or `q-waiting` objects, else canonical). Both names work in priority and focus/status filters; `p0` has no legacy name; logs keep `waiting`. Canonical writes on existing boards need all-host compatible release/idle approval; legacy objects and closed issues stay, only open issues are reassigned.
- `host_setup`: the project's one-time host setup, a path relative to the root of an executable that `service install` runs there (for example `scripts/install-git-hooks.sh` for shared git hooks); success writes `<root>/.taskq/host-setup` and clears the `host-setup` gap ([setup check](setup-check.md)). Workers never run it.
- `publish` `direct|pr`, `delivery` (§ 7); `workspace` `external` keeps host-owned worktrees; TaskQ's own `.worktrees/taskq-<N>` and `review-<N>` are still tidied ([worktrees](worktrees.md#tidy-pass)).
- Host-owned `<install>/host.json`: `hosts` (hostname: name; TASKQ_HOST overrides), `has` (name: capabilities), runtime `limits`, `admission` {"max": n}, `load_gate`, `codex` args, `permission_mode` (R9), `channel` (§ 8), `claim` `any|labelled` (default `any`, § 4; `add` defaults `--host` to this host), `default_runtime` `claude|codex|antigravity` (default `claude`; [detail](host-service.md#default-runtime)), `projects` {root: {limits}}, lower-only ([remote](shared-resource-controls.md#project-overlay)). No capacity_hosts/mode. Keep Auto/D03 defaults; migration enables no gate, changes no permissions.
- Design, UI, visual and image work goes to codex with `add --runtime codex` ([specq](https://github.com/taskqorg/specq/blob/main/engineering/agent-workflow.md#runtime-preference)); `add --prefer` is refused, hinting `--runtime`. Opt-in `codex-app` (macOS, Windows): only on hosts whose `has` lists it, only for `run-codex-app` tasks (`add --runtime codex-app`); reviewer chain checks keep app-worker cases on macOS and Windows and none elsewhere. A screen task adds `needs-screen` (`has` `screen`). macOS evidence tasks: [detail](../runtime-qualification.md#macos-evidence-tasks).
- Project taskq.json keeps board/repo/board host, publish/delivery, workspace, labels, recipes, incident_reports, pages, assignee and other project keys; `limits` per § 3. No host identity, capability, model/permission or admission copies. Token lookup and worker-policy grants (§ 8 Access) stay unchanged; host flags never grant access.
- `deploy` `[env, ..]`: environments in deploy order, e.g. `["dev", "live"]`; absent, closed tasks get no deploy line or `deploy-waiting` label and `taskq deployed` refuses (§ 2 Deploy).
- `recipes` `{name: {"version": V, "tasks": [{"title", "goal", "acceptance": [..], "scope", "deps": [earlier 1-based index..], "gate", "type", "runtime", "priority", "host"}]}}` (§ 7).

### Six-copy migration

- [Six-copy migration](host-service.md#six-copy-configuration-migration) compares inputs before writes; conflicts/unknowns hold launches. Keep originals, unknown keys and ownership; replacement resumes idempotently.

### Publication cutover and contract cutover procedure

Publication cutover: keep results, stop old services/children, set public GitHub queues/required `merge_group` CI, upgrade hosts, resume.

An agent reads entries from the board's version to the new one, plans each open issue and asks the owner about issues the new format cannot hold. After the owner's OK, with old services and children stopped, it applies the plan with `taskq` commands and issue edits. Closed issues are never touched. New services start: Mac, then Windows. Repair: the same procedure for one project, warning when `status` shows another host's fresh heartbeat.

### Antigravity subscription runtime

`antigravity` uses the `agy` streaming CLI for workers and reviewers. Host/project `limits.antigravity`, `default_runtime: "antigravity"`, `taskq add --runtime antigravity` and `taskq service capacity --host H --antigravity N` use the same admission and lower-only project limits as other CLI runtimes. Zero disables; an absent host limit has no ceiling. No configuration edit creates a permission grant or login.

Unpinned work follows [§4](../../taskq.md#4-service): preferred CLI, then eligible Claude, Codex and Antigravity in that order. Strict `run-*` pins never fall back; `codex-app` stays explicit. Each candidate retains its own model/effort settings and grant; Codex options are never translated into Antigravity options. Quota observations hold availability separately from operator capacity and never rewrite limits. Owner/operator sessions keep their existing routing.

## Balancing

Balancing separates Task routing, Runtime availability, Host balancing and Project policies. Existing `work` route/JSON aliases remain supported.

Optional project `routing` is an ordered array, for example `[{"labels":["ui"],"prefer":"codex"},{"labels":["code","3d"],"prefer":"antigravity"}]`. Each rule needs a non-empty list of labels and one CLI runtime preference. All listed labels must match; the first matching rule wins. A single `prefer-claude`, `prefer-codex` or `prefer-antigravity` task label overrides these rules for workers. Reviews use the host default. Explicit `run-*` pins override both for both roles. All soft preferences retain subscription fallback and each candidate's grants, capabilities, model/effort and capacity checks. Conflicting preference labels on an existing board task are ignored with one log line per pass; new task creation refuses them. Invalid routing refuses new admission.

**Compatibility:** `prefer-codex` used to be a strict worker pin and `prefer-claude` was ignored. Upgraded hosts treat both as soft preferences; existing tasks can therefore use another eligible CLI. No board labels are rewritten. To retain a strict choice, the owner uses `run-codex` or `run-claude` before mixed-version admission; older hosts do not understand project routing or `prefer-antigravity`. Coordinate host updates before relying on those policies. Active attempts keep their runtime/account.

Owner-only `taskq reroute --from claude --to codex` changes matching strict pins on open ready tasks without unresolved execution, reporting each readback/refusal. It does not change focus, limits, accounts, grants or models and does not migrate work.

Provider/account quota windows live in the existing host-wide account availability record, separate from operator enable/disable and slot limits. Retain overlapping observed windows; unknown reset uses bounded probes, never an inferred weekly reset. Every project excludes exhausted accounts; a spare account precedes runtime fallback. All exhausted means automatic wait with account/window/reset or next-probe evidence. Restart retains holds and probe ownership; missing or corrupt observations remain unknown. No paid API fallback, login creation or credential copying.

### Balancing cutover

Routing fences and reviewer quota continuation change older folds and require the coordinated project cutover in §8/§9 before new entries are written. This includes fenced `taskq: reroute` intents, `rerouted` completion, claims carrying `routing-revision`, and reviewer `steer quota attempt`. Historical unfenced reroute audits retain their meaning. Soft preferences and project routing also require compatible admission policy on every board host.

1. Inventory the configured board hosts and open tasks, including pending routing intents, results, retries, quota windows and native execution ownership. Unknown or unreadable ownership blocks replacement; it never proves exit.
2. Under the existing owner-approved cutover procedure, pause new admissions. Preserve active work until it finishes or a qualified native handover preserves its attempt, session, account and grants. Never replay, migrate or duplicate uncertain execution.
3. Upgrade and verify compatible CLI readers and services on every board host before allowing the new entries. Do not run mixed readers. Preserve strict pins, focus, configured limits, accounts, permissions, independent quota windows and pending probe ownership; no label rewrite or live policy change is implicit.
4. Resume in the existing approved order only after all hosts read the new model and retained state correctly. Read back new claims and continuations against the accepted golden traces; qualify actual native behavior separately under R12.

After these entries exist, rollback requires a compatible reader. Retain board logs, pending routing intents, quota journals and execution evidence; never delete or edit them to make an old fold appear compatible. Unknown completion stays fenced and follows the existing §9 repair procedure. Documentation and portable checks do not prove a live cutover.
