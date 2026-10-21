# Changelog

Per release: what changed on the board and what each open issue needs. A cutover agent reads this (taskq.md § 9);
there is no migration code. Releases without a **Board** section need nothing.

## Unreleased

## v4.6.27

Windows update fix (#1419): the update selftest of a staged release runs in a private telemetry store, so a running service that owns the telemetry writer no longer fails every install with `telemetry flush unacknowledged`; Windows hosts stuck on 4.6.25 install this release. No board change.

Linux sandbox: Codex workers keep `~/.codex/auth.json` writable, so a refreshed login persists and later turns no longer fail with 401 (#1424). A Codex login burned before this release needs one re-login. No board change.

Host log: each host line reaches `taskq.log` once (#1416). Idle heartbeats are not rewritten when only observation times change (#1358). The tray app uses the centered tray Q as its native app icon (#1347). No board change.

## v4.6.26

Mac dev channel (#1388, #1406): the Mac service of the `taskqorg/taskq` project builds and installs each new `main` SHA with green `merge_group` CI, using `scripts/dev-install.sh` of that fetched SHA, never the root working tree. Re-exec into a new build no longer waits for Claude background turns that have a saved native identity; the new image adopts them without resubmitting (#1407). A pending host restart re-execs once overlapping project passes drain (#1402). Install, dev install and the DashQ resources send wait a bounded time for the resources lock instead of failing on the first miss, so a dev install completes its upgrade without a manual `service install` (#1415). No board change.

Done rule for taskq self-fixes (taskq.md section 7 rule 8, #1408): a task closes on merged PR and green `merge_group` CI; live host checks run as one owner batch before each release, recorded in the version PR. Publication closes on green `merge_group` CI even when the later `main` push run fails and names the ignored failure; a merged-on-failed-CI or dequeued PR asks the owner in one pass (`publication-blocked`, #1413). Older services ignore that ask and keep polling. No board change.

macOS: `<install>` stays a plain folder and the owning service retires a `resources` folder an older host recreated, so the Screen Recording grant matches the launcher (#1395). No board change.

Host setup session runs without registered projects (#1399); the tray shows one setup row with its kind, reason, next step and Open session (#1400). The ask card shows the PM pick and reason beside the worker's star (#1390). The tray shows running agents by host and runtime (#1349) and selects several task states with checkboxes (#1350). No board change.

Telemetry: the service reads the telemetry store once per repair interval (#1341); receipts never block or fail commands (#1351); `taskq profile health` shows service telemetry health (#1345); selftest warns on historical terminal-record gaps instead of failing (#1389); local telemetry syncs to DashQ under an opt-in grant (#1353). No board change.

Board and publication: pause on GraphQL `RATE_LIMIT` replies (#1356); fewer repeated reads for publications and claims (#1359); enqueue merge conflicts count as main moved (#1372); queued publication qualifies on `merge_group` CI only (#1343); pending reviews are admitted before ready tasks (#1365); executions are released when the owning host booted after their intent (#1386); `taskq add` reports every text limit violation in one error (#1346); `profile tasks` shows ready entry time and merge queue state (#1344). No board change.

Runtime CLIs are found under `~/.local/bin` on Linux and unresolved runtimes are refused (#1393). CI runs Linux only on PRs and the full matrix in `merge_group`. No board change.

A task that closes without merging its PR (cancelled, superseded, delivered elsewhere) no longer leaves the PR open: each GitHub service that sees the close dequeues it, comments once with the task link and close reason, and closes it unmerged; the `taskq-<N>` branch stays (#1374). `requeue`, `later` and `failed` keep the PR open. No board change.

## v4.6.25

DashQ receives the `taskq.resources/1` project-limits projection as record `taskq-resources/1` beside the board snapshot (#1299; [enrollment record](docs/design/shared-resource-controls.md#project-overlay)). Existing connections keep sending the board alone until `taskq enroll` is rerun and approved in the browser; host resource grants name the connection generation and may need to be made again. No board change.

Telemetry pruning removes old rows in bulk under the profile lock, so a full store no longer stalls appends for tens of seconds (#1321, coreq pin). No board change.

`scripts/dev-install.sh` installs a newer local build under a running host service and waits until the service runs it (#1306); its rollback hint needs one `service restart` for every project. No board change.

The service pass no longer waits for DashQ connection storage (#1328): on Windows each CoreQ connection load ran `whoami` and `icacls`, up to four times per pass before the delivery thread started. The pass now reads `connection.json` only; stamping, the resources record and the unchanged-content check run on the delivery thread. A storage failure while stamping now reads `stopped: not sent: <reason>`. No board change.

Host update channel (#1325): `taskq service channel dev|live` saves `channel` in host.json and reports it applied after the host service reads it back; with no argument it prints the channel. On `dev` the update check installs no release and only re-execs into a newer local `current`; on `live` (also when `channel` is missing) it installs the latest release, also over a higher `-dev` build. `scripts/dev-install.sh` sets `dev` before installing and prints the channel; switch back with `taskq service channel live`. No board change.

`taskq service channel dev|live` accepts its value; it was refused as an unused positional argument, so `scripts/dev-install.sh` stopped before installing (#1335). A host that starts while the tray still owns a stale telemetry writer retries the collector each pass instead of admitting nothing until a manual restart (#1336). No board change.

Local macOS builds are signed like releases before their first start, so cargo runs and dev installs keep the granted privacy permissions (#1329). The tray header stays fixed on scroll and carries icon controls for Home, Settings, close, pin, Top and a size anchor; zero summary counts are hidden (#1326). No board change.

## v4.6.24

Unlabeled tasks and every review run on Claude (#1314). Work waits for a free slot of its runtime instead of moving to the other one: an unlabeled task no longer starts on Codex, and a review no longer falls back to Codex or Codex app. `taskq add --prefer` is refused with a hint to `--runtime`.

**Board:** existing `prefer-codex` labels read as `run-codex`, `prefer-claude` as no label; nothing to edit. Hosts on older versions still route by the previous rule until updated.

UI acceptance no longer requires native click and raise proofs; the owner observes them (#1298). The Windows handover selftest waits longer than one bounded telemetry append (#1305). No board change.

## v4.6.23

A native revision-bound workspace shows full AI exchanges (#1275); tray task rows keep their order and show the age of the current state (#1302). A new host recovers telemetry from a retiring older tray writer instead of reporting the command receipt collector unavailable (#1311). An older host treats an exited newer host's service record as stale instead of waiting on it forever (#1312). The spec now routes unlabeled work and every review to claude and keeps codex for design, UI, visual and image work (#1313); dispatch still follows the previous rule until #1314 ships. No board change.

Releases come from the `live` branch: a fast-forward of `live` to a `main` commit whose merge-queue CI passed tags and publishes; `main` pushes no longer release. The Mac development host installs local `<next patch>-dev.<epoch>` builds with `scripts/dev-install.sh`. Release builds use thin LTO and CI caches Rust dependencies. No board change.

A new process can bridge a running v4.6.19 telemetry writer with its exact legacy budget in the same database (#1295). Update candidate selftest and install receipts retain the shared export path; no handover side store is created. Unknown budgets, unsafe endpoints and unsupported wire formats remain refused. Compatibility lasts for that process; a fresh process after the old owner exits requests the 1 GiB budget.

Steady service, host and tray polling no longer writes one telemetry row per poll (#1297). Successful repeated work folds into 15-minute aggregates (coreq#63), while failures, changed results and command-linked passes stay detailed. Collector health writes one row per project loop per 15-minute window, or when its state, lost or failed-batch count changes; status.json keeps live values. The host name is read once per process, so a renamed host needs a service restart. A busy installation budget lock now delays a telemetry batch instead of losing it. No board change.

Recovery for v4.6.19–v4.6.22 hosts whose update fails with `telemetry IPC installation budget mismatch` (#1294): retry `taskq update` after this release is published, or run the new binary's `service install`. A full/unavailable old store can still prevent selftest or installation receipts; compatibility does not fabricate successful writes. The new Stop lifecycle can retain its durable intent and report telemetry receipts unknown while still requiring exact ownership, drain and native exit. Stop protocol 2 identifies that support; a new CLI refuses a degraded request to protocol 1 before writing intent. Older CLIs read protocol 2 as unknown; use the new CLI. Complete the service upgrade before using degraded Stop; exhausted legacy Stop remains safely refused. Released old CLIs are unchanged. No board change.

DashQ receives the `taskq.resources/1` project-limits projection as record `taskq-resources/1` beside the board snapshot (#1299; [enrollment record](docs/design/shared-resource-controls.md#project-overlay)). Existing connections keep sending the board alone until `taskq enroll` is rerun and approved in the browser; host resource grants name the connection generation and may need to be made again. No board change.

## v4.6.22

Telemetry keeps 30 days within a 1 GiB budget with half held in reserve; pruning and derived limits keep the service running instead of rejecting telemetry with `budget-exhausted` (#1278). Mac Drain and Full stop keep a durable hold (#1214), and after a reviewed Stop the owner can resume only selected local tasks (#1285). Mac activation follows [the repair handoff](docs/repair-1276.md); release publication does not prove activation.

**Board — cutover required (§ 9, #1279):** `usage-limit` ends a pre-work quota-limited attempt without spending retries. Old readers ignore it and retain the attempt. Stop old services and children, preserve open results and retries, upgrade every board host, then resume (Mac, then Windows). Do not run mixed readers or roll back to a reader without this event after it is written; reconcile open issues under § 9 first. Closed issues stay untouched.

## v4.6.20

Refresh native legacy inventory before it expires, retain fresh proof during refresh, and keep pre-launch admission waits out of task retries. Mixed telemetry snapshots acquire the budget lock before SQLite; independent command fixtures isolate inherited trace context. No board change, policy change or new grant. Mac activation and rollback follow [the repair handoff](docs/repair-1276.md); source checks do not prove native activation.

## v4.6.19

Integrates the reviewed host service, SQLite telemetry and CoreQ 0.14.4 (`5fb10926`), retaining the inventory-refresh wake repair. Mac activation uses the bounded [operator handoff](docs/release-1233.md); release publication does not prove activation. Merge-queue policy enablement remains gated by #1244 and actual operator evidence.

**Board — cutover required (§ 9):** GitHub PR publication saves `publish`, `enqueued` and private-fallback `rebased` entries; `publication-failed` counts failed CI and `requeue main-moved` preserves retries. Old readers fold these differently. Stop all old services and children, preserve each open result, configure public boards' main merge queue and required `merge_group` CI, then upgrade all hosts before resuming. Closed issues stay untouched. Private boards without a queue need Git and the clean, owned task worktree only for the fallback; public boards remain Git-free. Publication closes on the observed merge event with green merge-result CI, not queue admission.

## v4.6.18

Stop retains each idle project's verified preparation while other projects drain. Repeated wakeups cannot reopen the barrier; active operators, native executions and unknown ownership still prevent stopping. No board change.

## v4.6.0

Host service policy, exact execution ownership and command receipts share the supported CLI/tray paths. Host resource observations use the actual host revision; remote host mutation remains unsupported. Board comments remain task truth.

JavaScript uses ordinary shared modules with built-in bounded telemetry. Live capture and reports use the shared CoreQ store, preserving legacy import and original receipt-clock retention. Closed diagnostic output cannot interrupt native cleanup. Board snapshots reduce each unchanged history once. No board format change.

## v4.5.0

Local telemetry uses bundled SQLite/WAL, with FULL durability, 30 days from receipt and a 64 MiB total bound including database sidecars. All five record kinds, reports and JSONL export keep their schemas. `taskq profile guide` prints the embedded offline analysis guide. Collection needs no server connection. No board change.

Legacy history imports transactionally without deduplicating equal observations. The `telemetry.log/` namespace survives older updater cleanup, preserving SQLite and its sidecars during rollback. Unrecognized legacy rewrites, truncated telemetry and corrupt/newer databases remain unknown and are preserved; new SQLite collection continues when only legacy import fails. Before downgrading, keep this version's reader or export retained history: old readers see only the legacy log. Details: [storage transition](docs/design/standalone-telemetry.md#sqlite-implementation-and-transition).

## v4.4.4

Worker and reviewer briefs park unchanged external blockers with the failed check and one unblock condition. Resume only after evidence that the condition passed; repeated attempts are not recovery. No board format change.

CLI and service updates serialize under one install lock, preventing concurrent metadata/activation changes. Installation cleanup preserves this lock, the emergency hold and admission store/lock. Older versions delete these during an update: pause every registered project before updating from v4.4.3 or earlier, retain the hold outside the install, and verify it after updating. Resume remains an owner action.

## v4.4.0

Services check for updates every 60 s instead of 900 s. A merged version bump now tags and publishes the release by itself, with cached builds. The rules header asks for one evidence line per claim. Tray task rows link Claude sessions on every host and show the remote mark before the runtime mark. No board change.

## v4.3.0

Worker, reviewer and operator starts now send a rules header (specq revision, cache path, precedence), this project's `taskq.md`, the ten specq core files and an index of the other specq rules and `docs/`; resumes send the header and index. Rules are pinned to specq `46f5f62` and cached in `<root>/.taskq/specq/<revision>/`; a missing or invalid file, or more than 128 KiB in total, refuses the launch. No board change.

`taskq profile export|tasks|board [--since 30m|24h|7d]` reads this host's retained profile log across all projects without the board: JSON lines export, time in state per task, and board operations with failure rates and the lowest rate-limit remaining. Stage and transition lines now carry the project. Unknown values read unknown with a reason. No board change.

## v4.2.0

The operator workspace, pinned tray window and native notifications bring task questions, results and recovery into the tray. Services add host admission controls, worker access grants, runtime preferences and attempt profiling; delivery adds owner approval gates and reusable task recipes. DashQ connection and shared project controls use the same board-backed task state.

GitHub services and workers now use the owner-installed host board token, with no environment or CLI-login fallback. Provision it on each host before updating; a missing or unreadable token prevents board access. Never run `gh auth login` from a worker.

**Board**: keep tasks using owner gates or `review-pr` delivery off older services: 4.1.11 ignores approval entries. Enable these flows only after every board host runs a compatible version. Existing tasks need no edits.

The host heartbeat adds a `machine <hostname>[ wsl]` line (agentq `host.name`) so DashQ joins taskq and agentq into one Hosts row per machine. Older readers ignore the line. No board change.

`Depends on <repo>#N` waits on an issue in another repository of the same host (`name` under the board's owner, or `owner/name`); the task becomes ready within one pass of that issue closing. Waiting rows link each blocker: `blocked ([#N](url) open)`. A GitHub issue closed as anything but completed reads `unknown`, not done, and keeps its dependants waiting. A reply that is not that issue in a known state reads `unknown`.

**Board**: older versions read only `Depends on #N` and would start a task that waits on `<repo>#N` (taskq.md § 8). Until every host of the board runs this release, write a `<repo>#N` line only into a held task (`taskq add --hold`, then edit its text) and run `taskq start N` after the last host updated. Nothing to migrate.

`taskq tray --snapshot DIR --route R,..` captures only the named routes (`icon` for the icons); the capture window stays off the visible screen and takes no focus. README shots refresh on dispatch or a release tag; stale shots are a CI warning, not a failed check; a missing or malformed manifest or an unrecorded README image still fails. No board change.

Rollback below `4.2.0-dev` (4.1.11) loses worker grants: its update sweep deletes `<install>/worker-policy.json`, including on the update forward from it. Before a manual downgrade, copy the store outside `<install>` and do not downgrade unless the copy matches the original. Coming back: `taskq service pause` in each project, `taskq update`, copy the store back, and run `taskq service resume` only after `taskq service permissions` shows `Policy: revision N, M grants`; otherwise stay paused. No board change.

## v4.1.10

macOS summary tray icon: three numbers with minimal gaps, cadence graph across the full width, drawn at the full menu-bar height (33 pt on notched screens). No board change.

## v4.1.9

Worker and reviewer briefs name a contract that exists (the release's `taskq.md` URL when the project has none) and forbid searching `/` or `~`: workers no longer raise macOS Desktop, Documents, Downloads, network volume or Photos prompts for the service. No board change.

coreq v0.5.0: the launcher counts unhealthy starts per project, and sibling services claim an incident before reporting it, so one crash makes one issue. No board change.

## v4.1.8

Unused positional arguments now fail with their names before any action; use repeated `--acceptance` flags so every item reaches the task. No board format change.

Services sharing an install restart into a release already made current by another service, without another download or selftest. No board change.

## v4.1.7

GitLab OAuth logins (`glab` with OAuth) no longer get 401: the token goes as `Authorization: Bearer`, not
`PRIVATE-TOKEN`. Each board is its own module (`src/board/github.rs`, `gitlab.rs`) behind the `Board` trait; core,
service and operator no longer branch on the board kind. GitLab without `host` defaults to `gitlab.com`; setup writes
`host` for every board. No board format change.

## v4.1.6

TaskQ faults now produce sanitized, deduplicated public incident reports in `taskqorg/taskq`, through the existing
github.com account; unsupported auth, offline or uncertain delivery keeps a local JSON report. Setup and update
disclose the destination, account, payload and fallback. New setups enable this after owner confirmation; existing
projects without `incident_reports` stay local and show a notice. Set it to `false`, or `TASKQ_INCIDENT_REPORTS=off`,
to write nothing. The service status and tray show the newest outcome. Coreq is pinned to tag v0.4.0, including
launcher fault hooks and replacement on update (effective at the next launcher start). No board format change.

## v4.1.5

Same-host race: a worker that wrote `ask`, `result` or `requeue` and exits after the owner's answer arrived is a clean
exit, so its own service resumes the attempt with the answer instead of posting `gone`. No board change.
Verified live with mac and Windows on 4.1.5 (#844): attempt 6145455653 asked on mac, the owner answered, and the same
attempt resumed on mac with no `gone` entry.

## v4.1.4

Lease grace: an attempt younger than `ttl` by its `spawn` or its newest accepted `answer` or `steer` is judged only by
its host's heartbeat age, so the other host no longer posts `gone` for an answered attempt before its own host resumes
it. Both hosts need 4.1.4 for the fix to hold. An owner free-text comment on a task in `ask` becomes an AI request
whose proposal the owner confirms in the tray. No board change.

## v4.1.3

Update, launcher, service units, `status.json` common keys and the board token come from the shared crate
[coreq](https://github.com/taskqorg/coreq) v0.1.0; paths, units and messages are unchanged, except launcher errors
now read `taskq: <message>`. No board change.

## v4.1.2

Repository moved to `taskqorg/taskq`: releases, `latest.json` and `taskq update` come from there (the old URL redirects
for downloads; the board API does not, so `repo` in `taskq.json` must read `taskqorg/taskq`). R7 points at
`taskqorg/specq`. No board change.

## v4.1.1

In `ask`, a comment whose whole body is an option number or option text counts as `answer K`; 4.1.0 ignores it, so
such answers fold only on hosts running 4.1.1. Windows re-execs after an update once its live children end. Priority
`p0` passes any focus. The menu gains AI requests, Add project, an agents view and capacity: `taskq service capacity`
and `taskq agents`; `taskq.json` may hold `capacity_hosts` and a `codex-app` limit (old binaries ignore both).

**Board**: run `taskq labels` once to create `p0`.

## v4.1.0

One tray per host over every project in `<install>/projects` (heartbeat `<install>/tray`, errors in
`<install>/tray.log`); per-project trays of older versions exit once their service updates. `status.json` gains task
labels, `request`, `update` and `events_since`; `<root>/.taskq/events` keeps 30 days of close and failed moves.
`taskq service focus ... --request TEXT`, `taskq service pause|resume`; resume without a saved focus fails instead of
taking `all`. `<root>/.taskq/update` makes the next pass check for an update at once. No board change.
The tray icon shows run or pause (macOS may show the summary) and a click opens a webview popover: questions and
failed of all projects, project pages, pause/resume, log, folder, restart, stop and uninstall. View, text size and
period persist in `<install>/tray.json`; `taskq tray --snapshot DIR` writes its icons and popover pages as PNG.
`taskq add`, `ask`, `result` and `close` render and check the taskq.md message formats (§ 1 task body, § 2 ask card,
§ 7 verdicts): `ask --text` and decision results take `Done:`/`Problem:` lines, `result --text` takes `Changed:`/`Open:`
with one `--checks` line per check, a reviewed `close --text` takes `Changed:`/`Checks:`/`Open:`; over-limit text is
rejected. Old comments fold unchanged; no board change.

The history graph and the macOS summary icon draw 12 equal close/failed bins of 5 min, hour, day, 7 days, 30 days or
a custom `[start, end)` in local time or UTC; time before history is unknown, not zero. `Update now` appears when a
`status.json` reports a newer version and confirms it only from `status.json`. Pause all and resume all name each
project and its effect before acting; a mixed state reads `Paused N of M`.

## v4.0.10

Windows runs the tray as `<install>/taskq-tray.exe`, so a pinned icon stays pinned across updates. A tray on another
version than the live service exits and the service starts its own: the tray now follows updates.

## v4.0.9

Windows: the tray heartbeat and the poke never changed their file's mtime (Rust rewrote an empty file in place), so
the service started a second tray after each restart and slept a full `poll` after a local command. Both write a byte
now. The v4.0.8 "froze" was this: the tray ran, its heartbeat did not move.

## v4.0.8

Windows: the tray froze after its first pass (the event loop never woke on its timer); it now wakes from a timer thread.
Tray errors go to `<root>/.taskq/tray.log`.

## v4.0.7

macOS and Windows: `taskq tray`, a menu-bar / notification-area icon that the installed service starts and keeps running:
state color, counts, board and issue links, pause/resume, log, project folder, restart, stop until the next logon,
uninstall. New `service restart` and `service stop`; Windows `stop`/`uninstall` also end the service process tree.
`status.json` gains `pid`, `project`, `issues`, `log` and `tasks`. No reinstall: the updated service starts the tray.

## v4.0.6

Windows: workers, reviewers and the `gh`/`hostname` calls run without a console window (a console-less service got a
new window per child). No reinstall: `taskq update` and restart the task.

## v4.0.5

Windows: the v4.0.4 launcher exited with 101 under the Scheduled Task (stale console handles after dropping the console);
it now resets them, logs its own messages to `taskq.log` and keeps restarting the service. `service install` names the
task's user from `whoami` (an ssh session reports `WORKGROUP\<user>`, which schtasks rejects). Reinstall once.

## v4.0.4

Windows: the launcher supervises `service run` (a Scheduled Task restarts only a failed launch, so a crash or the
post-update exit left the host dead until the next logon), runs it without a console window (closing that window killed
the service with `0xC000013A`) and appends its output to `%LOCALAPPDATA%\taskq\taskq.log`.
Reinstall once (`taskq service install` from the new binary): the launcher only changes on install.

## v4.0.3

The host heartbeat carries an `at <board time>` line. GitHub keeps `updated_at` on a same-body edit, so an idle or
unchanged heartbeat aged forever and another host could post `gone` for a live attempt after `ttl`.

## v4.0.2

Windows `service install` registers the Scheduled Task for the current user, so it no longer needs admin ("Access is denied").

## v4.0.1

No token file needed: the board token falls back to the `gh`/`glab` login of the board host (§ 9).

## v4.0.0

Rust binary replaces `taskq.py` and `host_service.py`.

**Board** (v3 → v4), for each open issue except `taskq host` ones:
- Text: remove the block from `<!-- taskq:start -->` to `<!-- taskq:end -->`. Each id in its `deps` becomes a line
  `Depends on #N` unless the text already has one.
- State: v3 keeps it in the `q-<state>` label, v4 in `taskq:` log entries (§ 2). No entry is needed when the folded
  state already matches: `ready`, `waiting`, and `doing`/`review` (the v3 attempt ends with the v3 service).
- `later` / `failed`: `taskq later N --text` / `taskq failed N --text` with the text of the last `**later**` /
  `**failed**` comment; with none, ask the owner for the reason.
- `ask`: v4 has no `ask` without an attempt. Show the owner the question; the owner answers it in v3 before the
  cutover or the task is parked with `taskq later N --text <question>`.
- Only issues labelled `taskq` are tasks (§ 1): add it to every issue with a `q-<state>` label. An issue without
  either stays untouched; ask the owner whether it should become a task.
- v3 capacity issue (`<!-- taskq:capacity -->`): v4 does not read it; remove grants of closed tasks.

**Hosts**: a v4 service admits nothing until `taskq service focus all|<label..>` on that host (§ 4).
