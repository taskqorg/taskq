# Changelog

Per release: what changed on the board and what each open issue needs. A cutover agent reads this (taskq.md § 9);
there is no migration code. Releases without a **Board** section need nothing.

## v4.6.40

taskq pins coreq v0.14.10: a telemetry client waits 5 s for the writer's commit receipt and retries the same writer, so Windows service and tray stop losing batches (#1764). No board change.

TaskQ behaves as one normal app (#1758). macOS: TaskQ.app is always a Dock app with About, Hide, Restart TaskQ and Quit TaskQ in its app menu. Windows: `<install>\taskq.exe` with no arguments opens TaskQ without a console window; the Start menu `TaskQ`, its taskbar pin and the tray run it, so pins keep their path across updates, and the `taskq-tray.exe` copy is gone. The tray menu adds Open and Restart TaskQ. A tray of a temp install no longer rewrites the Start menu link. No board change.

Also in this release: drain Stop passes released attempts and keeps every project's status fresh (#1769, #1767); Conversations stay responsive on long history, keep reading and answer controls stable and retain threads (#1746, #1745, #1727, #1756, #1759, #1765, #1772, #1774); the tray loop stall no longer reads as Interface error and tray faults reach the heartbeat (#1760); the menu host strip, filters, View conditions and Completed rows follow the approved design (#1705, #1706, #1710, #1716, #1726, #1731, #1776); ask cards use short option labels, one render on every surface and inline images for visual choices (#1793, #1783); tasks added on a labelled-only host keep that host and `add --host` refuses unknown names (#1788, #1722); publication asks once and offers close with no merge when merged PRs supersede a result (#1778, #1780); a review without a leased reviewer shows as stuck (#1754); `requeue` refuses issues without the taskq label (#1737); `add` refuses acceptance lines typed with an A<n>. label (#1697). No board change.

## v4.6.39

Review uses the accepted purple on every surface that colors the Review state (#1677). A failing workspace route keeps the shell and request status stays on its conversation (#1680). Completed history loads in the tray and shows Merged only for a confirmed merge (#1679). Menu and Settings get the accepted titles, per-host counters, session marks and runtime versions (#1681). `taskq status` shows why a review waits for its reviewer (#1685). GitLab close waits for MR mergeability before merging (#1687). A worker park's blocker survives owner Later and Retry (#1689). No board change.

## v4.6.38

The task menu opens details on click only, keeps global task pins and per-view working state with saved views and one Undo, adds a Completed view read from board history (`taskq status --closed`) and uses the accepted state icons and colors (#1671). Settings is its own window with a stable header, Home and one Conversations entry (#1672). Settings shows hosts, agent sign-in and services as compact route widgets (#1673). PM requests and task answers share one composer, delivery state reads plainly and the ask parser rejects bad option numbering (#1674). The workspace dialogue gets the reply composer, transcript view and watch status (#1675). No board change.

## v4.6.37

`host.json` `default_runtime` (`claude` or `codex`, absent means `claude`) routes reviewers and workers without a `run-*` label to that runtime; `taskq service capacity --default-runtime` saves it and `service status` prints it (#1668). The workspace dialogue backend adds the owner turn, transcript reader and per-conversation watch (#1664). No board change.

## v4.6.36

The service tidies each root hourly and on `taskq cleanup`: closed-task worker and reviewer worktrees and merged branches go, a root on `main` fast-forwards only, every refusal names Git's first error line, and `taskq profile health` prints one tidy line per root; Windows removes deep `target/` paths via `core.longpaths` (#1654). No board change.

## v4.6.35

Stage records and the profile export (schema 3) split context tokens into input, cache read and cache write; `tokens` stays their sum, so DashQ can price attempts (#1647). No board change.

## v4.6.34

Releases ship the task workspace (menu, detail and views) as `taskq.task-workspace/1` js, css, fixtures and sources under SHA256SUMS, and the tray embeds the same bytes (#1645). `taskq profile health` shows live WAL bytes next to the WAL file size, read without the writer lock or a checkpoint; taskq pins coreq v0.14.9 (#1640). Runtime knowledge lives only in the adapter, enforced by check.py (#1593). Unrun previews and prototype tests, the unused enrollment reader and one-lambda wrappers are gone (#1599). No board change.

## v4.6.33

CLI, tray page, icon summary and notifications read one display model for counts, state names, order, ages and labels (#1592). A recovered Claude job that restarted on a new process resolves instead of staying fenced as unknown on Windows (#1638). The service logs exit reasons, shows stale hosts and restarts an exited Windows host (#1628). PM requests are shared through one composer, and native-session dispatch is fenced (#1615, #1636). taskq pins coreq v0.14.8: GitLab boards read `<install>/gitlab-token`, and WAL checkpoints are bounded (#1637). No board change.

## v4.6.32

Claude executions recovered after a Windows service restart resolve instead of staying unknown, so tasks blocked on them move again (#1629). The update re-exec waits a bounded time for the update lock (#1626); taskq ends on Windows without ExitProcess hangs or lost exit telemetry (#1574). Each ready task publishes a reason-coded admission state (#1632). Telemetry sync status and stage model fields are sent, with a published wire fixture (#1631), and request count keys are DashQ-valid (#1624). DashQ session progress carries host, runtime, stage and age only; worker line text no longer leaves the host (#1617). New task worktrees start from a cloned target/debug, task children share a host build cache, and concurrent cargo builds are bounded per host (#1623, #1619, #1618). The Summary SVG is shared and Custom icons are gone (#1611). No board change.

## v4.6.31

A pending restart no longer waits on answered or reserved tasks without a child: they pass to the new image, which resumes them from the board (#1601). The native legacy inventory, `service complete-legacy` and the other per-project migration bridges are gone, so Windows launches are no longer held by inventory timeouts (#1596). The tray gets the approved task dashboard and settings (#1578) and the Conversations journey with native-only drafts and attachments (#1580); CLI, tray and menu share one freshness rule (#1591); any worker session opens in one click and sessions are named by role, task and title (#1565, #1564). Runtimes live behind one adapter each (#1562). Recovery records stay on their own process and exited ones resolve at handover (#1561); a refused Claude launch counts as a confirmed no-start (#1557); native history stays observed on an mtime-only touch (#1588). `taskq add` refuses repeated single-value flags (#1558); onboard reports Claude trust gaps without closing Claude (#1555); profile counts only failed publish calls (#1572); history bars show exact interval counts (#1586). Merge-queue tests run on recorded GitHub responses (#1595). No board change.

## v4.6.30

`taskq service onboard` readies a host's Claude folder trust in one command (#1546). Closed tasks release worktrees held by foreign or rebooted executions (#1549). A label fix that another host's fold reverts backs off, is logged and reported once (#1550). Steady tray tick telemetry folds into aggregates, so the store keeps its full retention (#1548). Codex setup sessions open through the host service (#1534); the tray warns when agentq is missing and setup sessions need one OK (#1544). The service never dequeues a newer publication of the same SHA and reaps an adopted zombie child (#1537, #1543). Tray and status task rows show only owner texts; the tasks list gains a per-row menu and chip-order sort; the workspace shell runs as Preact route components (#1524, #1523, #1538). CoreQ v0.14.6. No board change.

## v4.6.29

Claude background sessions get their own launch env through `--settings` env, so a Claude daemon started by another process no longer leaks its `TASKQ_*` values into workers (#1531). The tray Back stack follows recorded Wouter history and obsolete tray and workspace shell code is gone (#1470). No board change.

## v4.6.28

Queue: every new task starts held; only `taskq start` (or a person adding `taskq`) admits it (#1475). A worker parks on a blocker task with `later N --blocker #M`: the task waits with `Depends on #M` and returns to ready when the blocker closes (#1486); 4.6.27 hosts read that park as `later`. `service capacity --claim labelled` makes a host claim only its `host-<name>` tasks (#1493). Status lists tasks stuck for 2 hours without movement (#1487). Heartbeat issues of retired host names close (#1483). Idle passes poll the board three times less often (#1431). No board change.

Publication and workers: a merge-queued PR is dequeued with GitHub's id input (#1472) and a queue removal with reason merged counts as pending (#1489). Workers read the worker contract at fetched `origin/main` (#1479). Ask cards render short labelled links and name the failing line (#1457). No board change.

Hosts and runtimes: each setup OK and each tray AI request opens a new visible session, and the operator turn is gone (#1426, #1427); the Remote Control link that appears after launch is recorded (#1409). Runtime login state is kept per host, with a PM re-login runbook (#1429); Codex is held at once when its refresh token is rejected (#1446). Host records and kids survive power loss (#1445). Windows update restarts pass stale unresolved executions (#1435); a remote execution its owner reports running needs no termination wait (#1491); status reads execution rows within the heartbeat ttl (#1448) and shows an accepted remote Codex worker as running (#1478). The Linux sandbox keeps the git common dir writable in workspace mode (#1490). The staged selftest no longer recreates its private store at exit (#1519). coreq pins carry the writer promotion and owner-scan election fixes (#1500, #1514); `profile` dates and attributes telemetry store stalls (#1517). No board change.

Tray and workspace: one navigation and control model (#1463); Held counter and drafts with Start, no Closed counter (#1476); toggling state counters and All tasks totals (#1438, #1455); view state kept across restarts and updates (#1439); the answer card writes nothing on an unchanged refresh and says why Send is disabled (#1430, #1456); AI requests, task answers and consequential operation reviews live in the workspace (#1464, #1465); live agent panels show before the full task list (#1466); a Messages list with archive and permanent record deletion (#1467). DashQ receives the shared task controls and scoped host online, login and queue observations (#1506, #1507). No board change.

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
