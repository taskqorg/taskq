# TaskQ operator menu

Approved layout; unimplemented capabilities remain design targets. Runtime follows [taskq.md](../../taskq.md).
[Shared principles](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md) · [Visual style](visual-style.md) · [TaskQ checks](operator-ui.md) · [AI requests](ai-workspace.md) · [Tray and workspace placement](tray-workspace.md): approved target TW1–TW8; where it differs, the clauses below describe current behavior until N/W/O/P/M ship.

## M1 Appearance

- Start compact. Show Icon / Summary / Custom on both macOS and Windows; apply immediately and persist. Square rendering follows I6; Windows compact-only runtime is a migration baseline, not the target.
- Fixed top strip: view, named host, admission state, icon-only pause/resume and Controls. Mixed/unknown state must remain explicit.
- Summary: fixed two-level square under [I1–I5](icon-summary.md); work = doing + review, answers = ask, third count = ready + blocked, cadence in the fourth cell. Hide zero counts without moving the other cells; use the corresponding K-A1–K-A6 counter colors. No active q/play overlay unless Summary would be empty; then show the existing compact Q.
- Pause keeps the same gray data and geometry with the existing compact q/pause centered. Partial pause is not full grayscale.
- Work is mint, questions peach, ready neutral, failures muted red. Unknown is explicit with cause; incomplete sums remain marked.
- Menu font options 14/16/18/20, default 16; typography and bounded-window behavior follow visual-style.md. Icon sizing is independent.
- HTML popover in wry (WKWebView/WebView2) with a Rust state/action bridge; no OpenAI widget dependency in production. Product content is English under R7.
- Overflow, unknown icon representation, bin count and Windows legibility remain I5 decisions, not permission to shrink arbitrarily.

Center the square Q body horizontally and vertically, excluding its descending tail; keep the whole tail inside the canvas. Apply this geometry to compact, empty-fallback and paused-overlay Qs at tray and app sizes.

The app icon reuses the tray renderer and its existing state logic, including the compact-Q fallback for an empty Summary (#1326). The macOS Dock/app switcher and Windows window/taskbar icons follow the live tray view; static Finder/TCC/notification identity and EXE resources use the compact Q. Package ICNS at 16–1024 px and ICO at 16–256 px. Keep `org.taskq.tray` and sign only after writing resources; live updates never modify the signed bundle. Pinned-window visibility stays governed by M13.

## M2 Scope and projects

- One tray per host reads the registered-project list. Never discover projects by scanning folders.
- Observation does not grant admission. Focus plus host constraints determines eligibility; counters still describe observed boards.
- Deduplicate tasks by provider, server, repository and issue. A project shortcut is not a scope filter.
- Home shows pinned and recent projects; All opens a searchable complete list. Initial visible count and recent ranking follow the proposals in adaptive-menu.md.
- Preserve pin order. Do not reorder under pointer or keyboard focus. Pause/empty state does not remove registration or pins.
- Attention includes every observed project, including those outside the shortlist. Missing/stale data remains unknown.

Counter scope, all states and Held drafts with Start: [K-A1–K-A6](task-counters.md). Aggregate strip sits immediately above Projects; project strips use the same order.

## M3 Navigation and tasks

Approved navigation/header and view-only multi-select targets: [UI01–UI04 and UI25–UI28](ui-rules.md#checklist-28-rules). The task-list counter strip alone toggles states with OR; all eight states show, zero as icon-only and unknown as `?`. Selected capsules expose `aria-pressed` and a visible outline. No Filter checkbox group. All tasks in that row counts deduplicated open tasks across all projects (Held included), clears state/project restrictions and shows each observed project with nonzero counters and its tasks; partial/unknown stays explicit. This control alone is exempt from the no-Work-total rule. Project-summary strips still hide zero. Filters never change admission focus or write the queue.

- Header outside the scrolling content: Home icon then Settings gear at left; size anchor, Top, pin and close × at right. No bottom Home or Settings navigation. Its slightly contrasting strip never moves during scroll bounce.
- Moderate rounded corners on every side except the edge attached to the menu bar or taskbar in popover mode; pinned windows round every corner.
- Home: fixed controls, Work mode, Attention, short Ask AI composer, project shortcuts and Settings. Target (TW2/TW3): panels with live figures, then tasks, then setting links; the composer moves to the workspace.
- Attention contains decisions, failed tasks and actionable service/admission problems under [A2](adaptive-menu.md#a2-explain-the-queue). Ordinary waits remain neutral.
- Project header: Back, name, Board link and compact nonzero counters. Target (TW1): one fixed bar with Back, title and actions, breadcrumbs `Projects › <project>`. Add is a plus in the Projects header.
- Task row: the first row holds state, board-provider mark, linked issue number, age, known host/runtime, session and other available links; global rows also identify project. The full title starts on the second row and wraps across the block width, never truncated. Age counts from the board time of the entry behind the last logged state change (`since` in the status row; Waiting and Held derive outside the log and keep it) in short units (`<1m`, `5m`, `2h`, `3d`); without a logged change it shows `age ?` with the reason. Open menus update ages in place every 30 s, without a redraw. Metadata may wrap when space is exhausted; links remain separate controls.
- Task row actions (#1523): no repeated action buttons in rows; one `⋯` menu at the end of the row's top line holds that row's task actions ([task controls](task-controls.md#component-and-fixtures)).
- Tasks list order (#1523): rows sort in the counter-strip order (Doing, Review, Ask, Failed, Ready, Blocked, Later, Held), then priority (`p0` first), then number. The strip and the rows read the one `states` list of [project blocks](project-blocks.md).
- Counter chips: one shared chip style. A zero chip centers its icon; a chip with a count centers icon and number as one group with equal padding. No per-chip offsets.
- Task row text (#1524): a tray row and a `taskq status` task row show only the owner texts named here; raw execution, fence and wait diagnostics stay in `status --json` (`execution_wait`, `executions`) and the service log. Rows carry `wait`, one of: `Waiting for a free slot` (admission or load gate), `Waiting for GUI access` (GUI eligibility), `Waiting for the agent to accept input` (input acceptance), `Waiting for your answer`, `Waiting for the previous run to end` (termination, cancellation, worker-to-reviewer handover), `Run state unknown; see service log` (missing, stale or unknown observation). Session text: `Session unavailable` or `Session starting or unavailable`, title `Codex session opens only on <host>`; status session cells `<host>: starting`, `<host>: running`, `<host>: Remote Control unknown`. Other row text is the state name, age, project, host, runtime and the R6 `blocked (...)` dependency reasons.
- Main task target opens the issue. Agent session and known PR links are separate controls; no nested links or double navigation.
- Codex/Claude marks identify the recorded runtime; codex-app uses Codex identity. Missing permalink: unavailable with reason. A Claude session link is account-scoped and shows on every host; a Codex thread link opens only on its owning host, elsewhere unavailable with a reason (Codex Remote cannot attach to live CLI threads). A non-local row shows the host name and a remote mark before the runtime mark, no remote text.
- Task detail preserves question, failure, work, ready, dependency and later access. Board links remain near the project name.
- Answer cards mount only for `ask`, never for stored cards on doing/review/ready/later/blocked rows. An ask with no parsed choices shows its original text, a requeue reason field, Later and Close. Close requires confirmation naming cancellation without publication, then canonical `later` and `close` with readback after each; no second step after failed/unknown parking. When the task leaves ask, its operation receipt leaves the card: accepted shows a 6 s overlay toast (×) that never moves rows and joins a collapsed Attention group `Answered` (newest 20, × each); unknown, rejected and stale stay as Attention rows, also after their project's task rows, with ×, unknown with the same-ID delivery check. Nothing stacks above Home; partial close names the parked task and board recovery. Target (TW2): the card opens in the workspace task-answer context, not in the menu; the same rules apply there.
- Back restores list/scroll position. Service, log, folder and admission controls always identify project/host.
- Current Agents is host-level, read-only: version/account or not installed/not logged in/unknown, plus probe age. Include disabled runtimes such as codex-app where reported.
- Agents row (shipped): one per § 5 `agents` runtime, `<runtime> <version> · <account>` or `· not logged in` / `· not installed` / `· login unknown`, with the age of the freshest `status.json` probe; none: unknown.
- Shipped popover (§ 4 Tray): Home, Projects, Attention and Settings, with Back restoring the previous route and scroll. Home shows scope, questions and failed of all projects (one task per board and issue), project rows that open in place with issue links and the board, errors and stale `status.json` apart from failed, and `Paused N of M` with the paused projects.
- Project page `Capacity` (this host only, other hosts unknown): per runtime `used/limit`, `Load gate: Off|Ready|Waiting: CPU|Waiting: memory|Waiting: recovery|Unknown: <reason>`; after the § 4 `Edit limits` confirm it shows `Saved; waiting for service` until § 4 `Applied`.

### Running agents

Home shows observed running workers and reviewers by host and runtime across registered project boards, deduplicated by board/task/execution. Use the same fenced `executions` as `status --json`, including their recorded role and runtime; task state alone never proves a running process. Each compact host row shows runtime totals; expansion shows worker and reviewer counts separately. Idle hosts occupy one line. Missing, future or over-60-second observations show unknown with age; partial project reads cannot prove idle.

Local capacity remains a separate observation: show occupied slots and held/requested reservations not matched to a running attempt, with its own age. Those reservations include unconfirmed executions, not just pending starts. Remote capacity is unknown without a local observation. Display Doing/Review task counts for reconciliation; neither task-state totals nor host-wide occupied slots must equal running counts. Host capacity can include other projects. Read-only rendering changes no admission policy. Acceptance: two hosts, a stale host, pending reservations, duplicate boards and incomplete data; both themes require owner approval before merge (#1349).

## M4 Request, interpretation and applied state

- You asked: original text from `service focus --request`; missing text says not recorded.
- We understood: short phrase derived from confirmed parameters and revision; do not maintain a divergent copy.
- Now applied: actual settings, freshness and differences. Pause preserves intent.
- Exact filters remain inspectable. Unsupported or ambiguous intent requires clarification; do not silently broaden scope.
- Natural-language matching and future-arrival policy require [UC-05/Q3](../use-cases/README.md#q3-accurate-natural-language-focus).

## M5 Admission and lifecycle

- Compact q shows play when at least one project admits; pause for a fully paused admission set. Observation-only is distinct. Mixed shows paused N of M, excluding observation-only projects.
- Pause stops new admission; current attempts finish. Resume restores saved focus; missing saved focus requires selection, never implicit all.
- A failed project action returns its reason to the menu and clears its pending indicator. The error remains through status refreshes until another action for that project; successful dispatch still waits for state readback.
- Global commands name affected projects/host and unchanged targets before apply. Per-project actions affect only that project.
- Show pending, applied, partial failure and unknown separately; confirm application by readback.
- Restart, pause and stop have distinct effects. Stop retains autostart; show its native restart condition and active-work consequences before use.
- Uninstall uses a nested confirmation naming the project, removes its service unit and preserves project files and other registrations. Remove shared install resources only when no consumer remains. R11 applies.
- Drain holds admission while current work finishes; Full stop cancels owned local execution, including offline. Both retain the host hold and remain separate from Pause; [E1–E5](emergency-recovery.md) define the reviewed-recovery candidate.

## M6 Cadence

- Four series bottom to top: close, failed, ask, blocked; event history, not CPU or current concurrency.
- Source: service-local `.taskq/events` cache; event time, task, kind and attempt ID (or comment ID for ask/blocked without attempt). Deduplicate IDs. Repeated failure after requeue is a new event.
- No board replay. Cache loss means incomplete history; show collection start and gaps. Unknown follows [unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy).
- Equal time bins, oldest left; presets use 12 whole epoch-aligned bins, the newest holding now and marked partial; the grid steps one bin at a boundary, never drifts. One linear scale across series and bins. Equal counts have equal heights; scale by the largest stacked sum, never 100% per bar.
- Default 24 hours; presets 5 min/hour/day/7 days/30 days/custom. Custom start < end, explicit timezone and half-open range [start,end); persist choice.
- Shipped: stacked from the bottom closes green, failed red, `ask` peach, waiting gray; custom `[start, end)` is typed in local time or UTC and start >= end is refused; the icon's 24 h uses the same 12-bin grid. Before the latest `since` and in projects without `events` the history says unknown.
- Period never changes scope or the current failed count. Expose precise totals/peak when small segments cannot resolve.
- Acceptance fixtures have nonzero first/last bars; real zero edges stay empty. Runtime keeps 12 source bins; the icon sums adjacent bins to the largest fitting divisor of 12 (I4/I6, D11). Evidence: [icon shift](evidence/icon-shift/README.md).

## M7 AI entry

- Target (TW4): the composer lives in the workspace and names the runtime (Claude or Codex, owner's pick when both exist). Current: Ask AI composer: project picker (registered projects), input, History/commands and Send. Send opens one new visible session in that project with the text as brief ([AI requests](ai-workspace.md#contract-detail)); no operator turn, conversation or confirm step.
- Under the composer the newest request shows `starting`, `launched` with `Open session`, or its no-start reason and next step. Closing the menu cancels nothing.
- History: last 20 distinct sent nonempty texts, newest first; selecting fills only. Clear affects history only. Suggested commands share the history control.
- An AI request is no task: its failure is not a failed-task count.

## M8 Implementation boundary

WS11 first slice (#976, accepted split): native Home / Projects / Attention / Settings and its five categories reuse the existing snapshot and action bridge. Back restores the calling route, disclosure state and scroll; refresh keeps the current route. Existing Work mode and Ask AI remain accessible; no new backend authority. Appearance exposes the currently supported view/font controls and reports the system theme.

### Shared view state (#1439)

`src/tray-view-state.js` owns navigation state for every tray route. Routes declare
valid identities and parents; common keys apply automatically. Views never access
storage. The module captures route (`page`), selected state filters (`taskFilter`,
OR; an empty array means all tasks), selected project (`aiRoot`), Work mode scope
(`working`), expanded groups keyed by stable `data-view-key`, and content scroll.
It keeps up to 32 Back entries with the same keys. It never persists task truth,
confirmation, pending operations, recovery selections, answer text or command drafts.

The native Settings adapter stores this as `navigation: {schema: 1, current, trail}`
in `<install>/tray.json`, through the existing atomic save and corrupt-file backup.
The native adapter refuses navigation payloads over 64 KiB.
Navigation/filter/project changes save immediately; scroll and disclosure changes
coalesce for 100 ms. Closing the popover flushes before Hide. All settings use this
one document and adapter; existing top-level keys and values need no migration.
The schema is independent of the release version. Service stop/restart leaves the
tray state intact; a restarted or updated tray hydrates once from its first native
snapshot. Later snapshots reconcile validity without overwriting live navigation.

Restore only registered projects, declared routes, known task states, finite
nonnegative scroll and boolean disclosure values. Drop invalid keys and obsolete
Back entries. A missing settings child falls back to Settings; a removed project
to Projects; an unknown route to Home. Drop a removed selected project or Work mode
scope. Missing groups keep their default; scroll clamps to the available content.
A bad/unsupported navigation schema defaults to Home without resetting other
settings. Snapshot capture explicitly overrides navigation and does not save it.
Save failure is shown as Settings unknown; it never reports persistence as applied.

Deferred follow-up contracts (not acceptance claims for this slice):
- Selective recovery: E-A1–E-A9, including durable hold, owned-process identity, unknown-liveness refusal and selected fenced restart with readback. Keep ordinary pause distinct.

F5 for this slice covers the native navigation/settings ledger and preserved existing actions ([evidence](evidence/ws11/README.md)). Full AI/recovery rows remain Not tested; no disabled placeholders count as their completion.


- Installed/source inventory: [current-menu-audit.md](current-menu-audit.md). Historical phase numbers are not evidence that features are absent.
- Preserve all existing functions while replacing layout. Shared tray aggregates current per-project data; service ownership follows the current runtime contract.
- Add project is implemented in the current contract; new AI-window placement changes its presentation, not setup safeguards.
- Old-project marker: taskq.json with failing selftest, accepted in #766. Diagnose before any cutover; do not overwrite existing configuration.
- Native accessibility and rendering require platform evidence; HTML tests alone are insufficient.

Native sizing, scroll ownership and cadence rendering follow [R-UI1–R-UI6](rendering-contract.md).

`taskq tray --snapshot` routes: `home`, `actions`, `project`, a Settings or top-level route, `icon` (through the renderer: both themes, run and pause, named by size), and `scroll`: Home with each bounded AI block at its start, middle and end, captured only when named.

## M9 Settings and maintenance

- Categories: Appearance, Activity history, Agents, Services and About. Work mode remains a direct entry, not a second editor.
- Appearance contains view/font; Activity history contains M6 presets/custom range; Agents retains host probes; proposed permission controls follow [P1–P9](worker-permissions.md), pending authority/enforcement approval.
- Services lists targets with freshness, incidents, log/folder, restart, stop with its native restart condition, and nested uninstall confirmation. Retain Resources access to the existing external monitor and incident consent controls. No repeated service sections on home.
- Stale status does not prove a dead process. Known logs remain accessible during failure; unknown log paths are explained.
- About shows tray/service versions and host. Update now appears only for a verified newer version; preserve progress, readback, failure and rollback visibility.
- Update writes the existing service request and follows §8. Do not claim completion from a button click.
- `<install>/tray.json` holds shared navigation state (M8), view, text size 14–20, period, the Custom icon, notification modes, cursors and open-task titles, Pin, Top, the chosen logical window size and its last position per display (off-screen: at the icon).
- Resources opens btop in its own OS terminal window (Terminal.app; Windows: conhost), movable, resizable and closable; missing btop is installed with `brew install btop` or winget `aristocratos.btop4win`, its output shown in that window; no package manager: one line saying so.

## M10 Add project

- Projects plus opens the OS folder picker; cancellation changes nothing. Show the exact selected folder.
- Empty folder without config: new setup. Existing files without config: preserve files, Git and ownership during connection/setup.
- Existing taskq.json + passing selftest: register/install through the existing confirmed flow; already registered is idempotent.
- Existing taskq.json + failing selftest: show first verified reason and repair/cutover route; no automatic replacement or migration.
- `Set up with AI…` (new or plain folder) writes a setup request with that folder as its only target ([setup](standalone-onboarding.md#contract-detail)); the session confirms account, repository, visibility and every external effect under § 6. New repositories default private.

Product choices and approval boundaries: [MP1–MP9](menu-product.md).

## M11 Remaining decisions

Canonical selections/default candidates: [D01–D14](decision-register.md). The following constraints remain until their implementation gates pass.

- Final shortlist count/recent ranking and attention grouping caps remain tunable proposals; stable control locations and attention-first layout are approved.
- Scope semantics, per-project budgets, urgent ordering and multi-repository workflow follow use-case Q1/Q3–Q6; do not infer new backend authority.
- Remote sessions remain informational; tmux is research, not a delivery promise.
- Emergency stop is required; mechanism and I6 overflow/rebin candidates require implementation qualification.

## M12 Shared Work mode

- One editor from home/settings/project; project entry preselects that project. Open it in the existing workspace (approved Workspace review), preserving menu navigation and drafts. Recovery uses the same workspace route.
- Named host; host limits and load guard affect all its registered projects. Project focus/filter/ceiling edits name their own selected projects and review; that selection never narrows a host edit.
- Selection defines project change targets, not registered observations. Unchecking leaves that project's focus/ceilings unchanged; host edits separately name their wider effect.
- Preserve runtime-specific host limits, including codex-app, and separately named lowering project ceilings. Disable unselected project fields; unknown values follow [unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy).
- Different current focus displays Mixed. Opening does not overwrite it. Change only explicitly edited fields.
- Review exact project/host old→new changes; current attempts continue. Cancel preserves settings; readback is per project, including partial failure.
- Preserve CPU/memory gate controls and saved-vs-applied distinction from the current UI. Gate semantics remain in taskq.md.
- Native tray (#954): Work mode exposes one host editor for maximum, runtime limits and load gate, without project-selection controls that could imply a narrower effect. Project entry retains that project's observation context. Fixed remains legacy-only. Use the [shared resource controls](shared-resource-controls.md#shared-component-930); project focus/filter/ceiling work remains a separate target until its contracts and backends are qualified.

## M13 Pinned window

- The menu header carries icon-only Pin and Top controls with tooltips and accessible names. Pin orientation distinguishes pinned from unpinned; Top uses overlapping squares. × hides either window. Unpinned (default): the popover closes on blur, as today.
- Pinned: the same content stays open as a normal app window in Cmd+Tab/Alt+Tab where supported (unsupported behavior requires an owner decision), with the same width rules; it moves anywhere by dragging its header, does not close on blur or on a tray icon click (the click raises it).
- The window snaps to the screen work-area edges where the OS provides it (Windows Snap); on macOS it keeps within the work area and remembers the last position per display.
- Unpin returns to the popover at the tray icon. Pinned state and position persist in `<install>/tray.json`; a missing or off-screen position falls back to the tray icon.
- Top independently keeps the window above normal windows (default on); with Top off it can go behind others and a tray click raises it. Pin and Top persist independently.
- The header stays outside the scrolling and rubber-band region and is the drag handle; controls act on the first click on macOS.
- Size anchor cycles Fixed size, Keep bottom edge, Keep top edge; its icon, tooltip and accessible name show the selected mode. Fixed keeps both dimensions; the edge modes fit content vertically while keeping the named edge, clamped to the work area. Persist the mode in `tray.json`; existing manually sized windows keep Fixed size, other windows keep top-edge content fit.
- A bottom-right resize grip changes both dimensions; the chosen logical size persists across reopen and restart. Before a manual resize, fit content at the current font and DPI up to the monitor work area: the fit height is the window's measured layout (header, chips, summary, list, footer and any added panel) with the scroll area at its full content height, never per-panel pixel constants; scroll only when content exceeds available space.

## M14 Notifications

- The tray posts native OS notifications (macOS Notification Center, Windows toast) for new board events read from each project's `events` (§ 4 Tray). A closed task keeps its title in its banner and card.
- Event kinds: `ask` (a question waits for the owner), `close`, `failed` and `gone`. Settings > Notifications sets each kind to Off, Silent (menu badge only), Banner or Banner with sound. Defaults: `ask` Banner with sound, `failed` and `gone` Banner, `close` Off.
- A project may override the host setting per kind (Inherit by default); the project page shows the effective value.
- A notification names the project, the task title and the kind; a click opens the menu (or the pinned window) on that task's card, for `ask` the answer card (target TW2: the workspace task-answer context).
- Once per event (§ 4 Tray): events at or before a project's cursor (the last notified event id), or older than the first tray start with notifications on, never notify. More than 3 events within 10 s become one summary notification per project.
- The first enable asks the OS for permission on macOS; Windows reads the OS notification setting without a consent prompt. While the prompt is open Settings shows `Asked`; a failed request shows its OS error, never `Not asked`. Denied or revoked: Settings shows `Blocked by the OS` with a link to the OS setting, and nothing falls back to another channel. `Send test` posts one sample of the selected kind.
- An unknown `tray.json` value reads as the default and keeps the file.
- Windows toasts use AppUserModelID `org.taskq.tray` on a TaskQ-owned Start Menu shortcut to the stable tray executable, whose toast activator CLSID names the running tray's COM activator, so a click on a banner or in Notification Center reaches it; OS settings are read through the notifier (denied: Blocked, no setting recorded before the first toast: the OS default, allowed, other errors: Unknown). A failed authorization request is logged in `tray.log`.
- Packaging: Windows runs the tray as `<install>/taskq-tray.exe`, so the notification-area pin follows the exe path across updates. macOS notifications show the TaskQ app icon, packaged in `<install>/TaskQ.app` (bundle `org.taskq.tray`, signed as taskq.md § 8 step 5), which notifications need, started through LaunchServices (`open`; the bundle with no args is the tray of its parent folder and appends its stderr to `tray.log`): a bundle exec'd by the service is refused authorization.

## Acceptance

| ID | Evidence |
|---|---|
| M-A1 | Compact start; view/font persist. |
| M-A2 | Deduplicated counts; observation never enables admission. |
| M-A3 | Project/Back and issue links reach the correct target. |
| M-A4 | Pause preserves focus; mixed scope is explicit. |
| M-A5 | Period affects history only, not scope/current failed. |
| M-A6 | Event IDs, half-open ranges, failed, zero and unknown remain distinct. |
| M-A7 | Clarification/confirmation/readback handle stale revisions and retries. |
| M-A8 | Closing preserves requests; failure never looks applied. |
| M-A9 | Original request, interpretation, actual state and mismatch are distinct. |
| M-A10 | Task failure, service fault and AI failure remain separate. |
| M-A11 | Lifecycle commands name targets and preserve unrelated projects. |
| M-A12 | All M9 actions retained; missing focus never resumes all. |
| M-A13 | History/suggestions fill only; history persists and clears independently. |
| M-A14 | Picker cancellation and all four setup cases preserve §6 confirmations and §9 cutover. |
| M-A15 | Incident states and none remain distinct; local report paths and issue links work; local is neither task failure nor successful delivery. |
| M-A21 | Attention precedes ordinary lists and includes projects outside the shortlist. |
| M-A16 | View/admission/pause together; mixed and target scope explicit. |
| M-A17 | Flowing title, header Board, separate session link: Claude on every host, Codex only on its owning host, elsewhere its DashQ progress page; remote mark before the runtime mark. |
| M-A18 | Pinned/recent, searchable All, header plus; all-state aggregate/project counters and Held with Start follow K-A1–K-A6. |
| M-A20 | Shared editor preserves Mixed, untouched fields/targets and per-project readback. |
| M-A21 | Pin keeps a normal app window open; Top independently controls above-normal placement; bounce-proof icon header, close ×, three size-anchor modes, first-click controls and a bottom-right resize grip; size, position and toggles survive a tray restart; unpin returns to the popover; Windows edge snap, macOS work-area clamp. |

[Verification](verification.md) separates historical evidence from the current candidate. General principles change per [shared-rule improvement](https://github.com/taskqorg/specq/blob/main/engineering/spec-first.md#improve-shared-rules); TaskQ behavior changes here under R13.
| M-A22 | Each kind follows its host or project setting; an event notifies once across tray restarts; a burst becomes one summary; a click opens the card; OS denial shows Blocked and posts nothing. |

## Contract detail

Moved verbatim from `taskq.md` (#1179) to keep the contract under its R8 size bound. `taskq.md` keeps the decision and links here; this text keeps the same authority (R8, R13).

- Tray (macOS, Windows; layout, routes and platform packaging: [menu specification](menu.md) M1–M14): one per host. Any installed service starts `releases/<current>/taskq tray` when the heartbeat `<install>/tray` (the tray's pid) is older than 30 s; a tray that finds another fresh pid there exits, as does a tray on another version than `<install>/current`. It lists the projects of `<install>/projects` and reads each `<root>/.taskq/status.json` and `events`: no board, no token. Icon: the compact q with ▶ when a project admits, Ⅱ when none does, gray when all are paused; macOS may instead show Summary or a saved Custom icon ([square summary](icon-summary.md), [custom icon](custom-icon.md); Windows: compact only, menu M1), both drawn by one renderer (`src/icon.rs`) over a versioned configuration; an invalid saved Custom falls back to Summary. Writes name their target first: pause/resume per project and over all after a confirm naming each project and its effect; resume without a saved focus offers the project's labels and writes the pick; per project log, folder, restart, stop until the next logon, and uninstall after a confirm naming it. `Edit limits` writes only after a confirm naming project, host, old → new and `Running tasks continue`, and shows `Applied` only when `status.json` reads the values back. `Update now` shows when a `status.json` names an `update`: it writes that project's `<root>/.taskq/update` and reports success only when a `status.json` reads back that version. History (M6) is built from `events` only. Agents (M3) shows the § 5 `agents` probe, read-only. Resources opens btop in its own OS terminal window, never inside the menu; missing btop is installed first with the OS package manager, unelevated (M9). Notifications (M14): native OS notifications of new `events` lines, per kind and project as M14 sets, at most once per event. Tray settings, cursors and open-task titles persist in `<install>/tray.json`. `taskq tray --snapshot DIR [--route R,..]` writes the icon PNGs and popover snapshots of the current state in both themes, then exits; an unknown route is refused (routes: M8); the capture window stays off the visible screen where the OS allows and takes no focus.
- Events: a pass appends to `<root>/.taskq/events` a line `<board time> #<N> close|failed|ask|blocked|gone <id>` per task it saw move to closed, `failed`, `ask` or `waiting` since its previous pass (a new task counts from its first state); the id is the attempt current at the move, else the task's last spawn; for `ask` and `blocked` else its last comment, else the issue number; a closed or `failed` task never spawned adds none. `gone`: per trusted `gone attempt <id>` log entry newer than the previous pass (the first pass adds none). One line per attempt and kind; lines older than 30 days drop. The first line `since <time>` is the first time covered (`events_since`); a restart keeps the file, moves while no service ran are missing. `ponytail:` no board replay. `ponytail:` files, no socket or pipe, on every OS.
- Session links (R6): Claude links use only the Remote Control URL registered by that native session; the local UUID is not a remote ID. Codex links on the owning host: `codex://threads/<id>` when `TASKQ_CLIENT=codex`, else `<pages>/open.html#codex://threads/<id>`; on another host the DashQ progress page `<origin>/projects#board-<repo>-task-<N>` of the active connection (R6 3), else the host name alone.
Owner-facing service/tray text follows R7: counts use R6 names/order; task rows are `#N <title>` (title at most 60 characters). Use `Focus: <labels|off>`, `Paused`, `Service: <running|stopped|unknown>`, `Update: <version|up to date|unknown>`; unknown/stale values per [unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy). Menu action labels at most 32 characters; summary/reason lines at most 160; navigation per the menu design and [D8](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md#principles); CLI lists wrap one project/label per line when needed.

A corrupt `status.json`, `tray.json`, `kids` or `focus` reads unknown (reason in `status.json` `focus_error`, the tray's project error and `Settings unknown` line, `service status`), admits or adopts nothing and deletes no release; a tray save first copies a corrupt `tray.json` to `tray.json.corrupt`.
