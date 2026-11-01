# Commands

Command syntax, task bodies, outputs, op ids and receipts.

## Contract detail

Linked from `taskq.md` [§ 1](../../taskq.md#1-tasks) and [§ 5](../../taskq.md#5-commands); this text binds like the contract (R8, R13).

### Task body written by `add`

`add` writes these sections in that order, then dependencies: Goal 1-2 lines; Acceptance 1-8 items, one line each, unlabelled (a typed `A<n>.` is refused);

Scope optional, 1-4 bullet lines; with `--op-id`, a last line `op-id: <id>` marks the create for read-only recovery. Title ≤ 80 characters; body lines ≤ 240. Short imperative sentences: Goal names the user outcome; each Acceptance a visible check; Scope a boundary. Headings on their own lines; one blank line between sections. More than eight checks: split before filing (R2).

### Command table

| Command | Does |
|---|---|
| `taskq labels` / `taskq add "<title>" --goal G --acceptance A [--acceptance B..] [--scope P..] [--deps N..] [--type T] [--runtime R] [--priority 0\|1\|2] [--host H] [--conversation ID --op-id ID]` / `taskq start N [M..]` | §6 labels; add: `#<N> held`; §1 start |
| `taskq finding <title> --board URL --source installation/rule/episode --goal G --acceptance A.. [--evidence E..] [--start] [--json]` | § 1 create/optional-start receipts; `add` routing/scope/dependency flags apply |
| `taskq ask N --text Q [--option O..] [--recommend K] [--link URL..]` | question card |
| `taskq answer N --text A` / `taskq answer N.K [M.K..]` / `taskq pick N.K --text R` | owner's answer or option pick, all checked first; `pick`: § 2 PM view |
| `taskq result N --sha SHA [--checks C] [--text T] [--option O..] [--recommend K] [--link URL..]` | hand in; full 40-hex SHA |
| `taskq requeue N [--text T]` / `taskq steer N --text T` | back to `ready` / text to the live worker |
| `taskq later N [--text T] [--blocker #M\|--until UTC] [--stage A<k>..]` / `taskq failed N --text R` | park (§ 7) / hold |
| `taskq close N [M..] [--text VERDICT]` | publish (§ 7) and close; one failure never stops the rest |
| `taskq approve N [--sha SHA]` / `taskq recipe NAME --run ID [--goal G] [--start]` | § 7 owner approval of the current result (refused with `TASKQ_ATTEMPT`) / recipe run |
| `taskq status [--only L,..] [--json]` / `taskq status --closed [--page N] [--since T] [--task N [--transcript [--before B]]]` | Read-only R6; `--only`: any label (absent: all). JSON/history: [menu](menu.md#contract-detail) |
| `taskq inbox N [--json]` / `taskq retro` | task text, folded state and log (a requeue says what to fix); `--json`: progress, answers `{by, at, revision, option, text}`; `retro`: owner session (§ 4) on use-case proposals and board `ask`/`answer`/`requeue` history, filing accepted ones via `add` |
| `taskq issue N [--comment T\|--label L] [--op-id ID] [--json]` | read any board issue, or add a comment or label: [detail](issue-command.md#contract-detail) |
| `taskq deployed --version V --env E --range A..B [--host H] [--json]` | [§ 2 Deploy](../../taskq.md#2-log-and-states): one `deployed` entry per closed task a `Task:` trailer in `git log A..B` names; op id `deployed-<E>-<V>[-<H>]`, so a rerun is a no-op; other boards' tasks are skipped and listed; `E` must be in the project's `deploy` ([keys](configuration.md#keys)). Release scripts (TaskQ's own update, pilotq, cs, dashq) run it in the project root once a version is live: `--version` the new version, `--env` the environment it reached, `--range` previous release tag `..` new tag, `--host` the machine or site. Lines `#N accepted\|repeat\|skipped\|unknown[: reason]`, `skipped <url>: <reason>`; `unknown` exits non-zero: rerun the same command |
| `taskq wait [--window MIN] [--only L,..]` | PM: block until a task moves to `ask`, `review` with options, closed or `failed`; print `ask #N` etc. or `tick`; folds issues updated since the cursor `<root>/.taskq/pm-cursor` (a cache, R1) |
| `taskq cleanup [--force] [--dry-run] [--all]` | remove clean worktrees and branches of closed tasks; `--force` also dirty ones (R11); every removal, refusal and kept task worktree prints with its reason; `--dry-run`: print what would go, change nothing; `--all`: each registered project of the host, one header line each ([worktrees](worktrees.md#commands)) |
| `taskq doctor [--dry-run]` | one [host doctor](host-service.md#host-doctor) pass now: old releases, stale temp folders and qualification objects, one line each; `--dry-run`: print only |
| `taskq update [--check]` / `taskq enroll <https-url> [--headless] [--telemetry]` / `taskq connection status` / `taskq disconnect` | update: § 8 (coreq), one sweep; enroll: approve board scope in a browser (`--headless`: print code), `connected` only once DashQ projects the first snapshot; status offline; disconnect, pairing, revoke, host observations, admission: [DashQ connection](enrollment.md#contract-detail) |
| `taskq service install [--cancel ID]\|run\|onboard\|focus\|pause\|resume\|hold [--text R]\|release\|recover [--apply] [--request JSON] [--json]\|status\|restart\|stop\|uninstall\|list\|setup\|check\|channel\|permissions [--runtime R --mode M\|none [--scope host\|project]]` / `taskq tray` | §§4/8; `install --cancel`: exact unused-attachment reconciliation ([H-A5](host-service.md#contract-detail)); `setup`: § 4; `check`: § 6; `onboard`: R9; `permissions`: §8 read or one grant write/readback; `hold`/`release`/`recover`: § 3/§ 4 (`release` refuses reviewed Stop holds); `restart\|stop`/`status --request ID`: exact host receipt (H-A5); `stop [--mode drain\|full]`: native stop, unit retained ([conditions](host-service.md#contract-detail)); `list`: host projects/focus; `channel [dev\|live]`: print, or atomic write, poke, `applied` on service readback; `tray --snapshot DIR [--route R,..]`: § 4 |
| `taskq service capacity [--host H] [--max N\|none] [--claude N] [--codex N] [--codex-app N] [--load-gate on\|off] [--cpu-max P] [--memory-min P] [--claim any\|labelled] [--default-runtime claude\|codex]` | local limits/use/gate; owner writes need `--host` (R9), atomic host.json/command binding, exact service receipt; no remote or mode edits; [detail](host-service.md#command-surface-acceptance) |
| `taskq service resources [--grant ID --actor ID --hours N [--project P].. [--runtime R].. [--host-policy] [--project-limits] \| --revoke ID \| --json]` | the host operator's [resource grants](shared-resource-controls.md#host-adapter), project IDs, revisions, receipts; `--json`: overlay projection; grant/revoke refused under `TASKQ_ATTEMPT`. A DashQ resource operation writes host capacity like `service capacity`, only inside an active grant of this connection |
| `taskq service app-server --action launch\|observe\|cancel --request JSON` | Protected external Codex stdio and separate authority readback ([protocol](protected-app-server.md#contract-detail)); no policy, login or capacity changes |
| `taskq service slots --identity PID` / `taskq service slots --action reserve\|observe\|release --request JSON [--json]` | Local manager slot [protocol](external-slots.md#contract-detail), host capacity only; no task or policy mutation |
| `taskq service pm [--grant ID --actor ID --hours N --project P.. --runtime R.. \| --revoke ID] [--json]` | [PM authority/receipts](pm-requests.md); owner-only, off by default |
| `taskq profile [guide\|export\|tasks\|board\|actions\|health\|migrate] [--since 24h]` | Local telemetry; no board; [detail](profiling.md#contract-detail) |
| `taskq agents` | read-only login probe per startable runtime: [runtime login](capacity.md#runtime-login) |
| `taskq accounts [--json]` / `taskq accounts enable\|disable <runtime> <label>` / `move <runtime> <label> <position>` / `add <runtime> <label>` | list every host's runtime accounts; change this host's list or add one by device login: [runtime accounts](capacity.md#runtime-accounts) |
| `taskq selftest` / `taskq --version` | load config, fold built-in `tests/golden`, read the board; no writes / [binary version](host-service.md#contract-detail) |

### Outputs, op ids and receipts

- Task writes: `#N <state>`; close adds one publication link/SHA line.
- Other acknowledgements: action, project/version; one line ≤ 160 characters. Capacity writes add `Receipt: <id>`. Batch: one outcome per item.
- [Human replies](answer-card.md#human-replies): `answer`, `steer`, owner `requeue`: text/images/files, no size/count cap.
- `service focus/list/status`: named project, focus, counts, freshness and unknown reason; one field/item per line, ≤ 160 characters.
- `update`, `cleanup`, setup and `selftest`: one outcome per version/path/check ([Core Write](https://github.com/taskqorg/specq/blob/main/caveman/rules/core.md#write)).
- Help keeps full syntax; `inbox`, raw service logs/status JSON and original issue/log text have no display caps ([verbatim diagnostics](https://github.com/taskqorg/specq/blob/main/caveman/rules/changes.md#keep-the-source-and-sink-separate)).
- Refuse unused positionals before actions: `unused positional arguments: <arguments>`; repeat value flags per item.
- `--op-id ID`, `--revision R` (answer, requeue, later, failed, steer, result, ask, close, start, approve; `add`: op id, §1): a seen id returns its first receipt; stale revisions refuse, except owner answers/steers save a §2 note with that id, nothing for a repeat of the winning answer. A raced entry edits itself first. Unknown delivery reconciles the same payload/id.
- Receipt: `--json` prints `{task, op_id, outcome, state, revision, reason, next}`; outcome `accepted` (readback state), `rejected`, `stale` or `unknown` (written, no readback: `Next:` reads `inbox N`, retries the op id); else the lines above. Late answers add `answered {by, text}`, like R6 rows outside `ask`; every surface reads `already answered by <by>: <text>` once. [Task controls](task-controls.md) `taskq.task-controls/1`: start/stop/later/retry, no close.
