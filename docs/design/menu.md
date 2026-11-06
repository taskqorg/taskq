# TaskQ operator menu

Approved layout; unimplemented capabilities remain design targets. Runtime follows [taskq.md](../../taskq.md).
[Shared principles](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md) · [Visual style](visual-style.md) · [TaskQ checks](operator-ui.md) · [AI requests](ai-workspace.md) · [Tray and workspace placement](tray-workspace.md): approved target TW1–TW8; where it differs, the clauses below describe current behavior until N/W/O/P/M ship.

## M1 Appearance

- Start compact. Show Icon / Summary on both macOS and Windows; apply immediately and persist. The shared SVG and palette A follow I1–I6; Windows compact-only runtime is a migration baseline, not the target.
- Fixed top strip: view, named host, admission state, icon-only pause/resume and Controls. Mixed/unknown state must remain explicit.
- Summary: six-bin event stack A and counters under [I1–I5](icon-summary.md); work = doing + review, answers = ask, third count = ready + blocked, six chronological Completed/New/Ask/Failed stacks below. Hide zero counts; I2 owns exact values and slot fitting. Use the corresponding K-A1–K-A6 counter colors. No active q/play overlay unless Summary would be empty; then show the existing compact Q.
- Pause keeps the same gray data and geometry with the existing compact q/pause centered. Partial pause is not full grayscale.
- Work is green, questions amber, ready neutral, failures red under palette A. Unknown is explicit with cause; incomplete sums remain marked.
- Menu font options 14/16/18/20, default 16; typography and bounded-window behavior follow visual-style.md. Icon sizing is independent.
- HTML popover in wry (WKWebView/WebView2) with a Rust state/action bridge; no OpenAI widget dependency in production. Product content is English under R7.
- Overflow, unknown icon representation and Windows legibility remain I5 decisions, not permission to shrink arbitrarily.

Center the square Q body horizontally and vertically, excluding its descending tail; keep the whole tail inside the canvas. Apply this geometry to compact, empty-fallback and paused-overlay Qs at tray and app sizes.

The app icon reuses the tray renderer and its existing state logic, including the compact-Q fallback for an empty Summary (#1326). The macOS Dock/app switcher and Windows window/taskbar icons follow the live tray view; static Finder/TCC/notification identity and EXE resources use the compact Q. Package ICNS at 16–1024 px and ICO at 16–256 px. Keep `org.taskq.tray` and sign only after writing resources; live updates never modify the signed bundle. Pinned-window visibility stays governed by M13.

- M1.1 Whole-window opacity: Appearance offers Off (100%) and a labelled 60–100% opacity slider in 1% steps. Apply through the native menu window alpha and persist in tray.json. Native OS chrome owns titlebar and Close; no application-drawn replacement. Text follows the [contrast palette](visual-style.md#translucent-menu-text). Separate windows and tray icon retain their opacity. Failures retain the prior value and remain explicit.

- M1.2 Background-only transparency: Appearance offers Whole window (M1.1) and Background only, default Whole window. Save each mode's independent values in `tray.json`. Background opacity is 0–100%, default 100%, in 1% steps. Retire readability backing; legacy `backing_strength` values load without error and have no effect. Background mode restores native alpha to 100%; all menu, block, card, chrome, footer and popover fills, decorative borders and dividers follow background opacity, with no backing floor, except the M3 sticky Home host strip and task-counter row, whose fills stay opaque; text, icons, buttons and badges retain full opacity, including unavailable controls. Keep separate windows and the tray icon unchanged. Application failures restore the prior mode and native alpha; save failures remain explicit. At 0%, only the desktop remains behind foreground content outside these pinned rows. Show the mode selector and only the active mode’s fields; switching modes preserves both saved values. Do not render an inactive slider or require resizing to reach the active fields. [Fill treatment](visual-style.md#background-only-fills).

## M2 Scope and projects

Project heading shortcuts retain direct project-overview navigation on the name; it never opens the folder, starts work or silently changes the saved task view. Board uses the registered provider and exact board URL; Folder uses the existing native action and registered root on this computer, never a worker worktree or inferred path. Missing root disables Folder with a reason; OS failures remain visible. Back restores list, filters, collapse choices and scroll. Each action has an explicit accessible name/tooltip and 32/44 px hit area.

### Project group headers

The [UI30 target](ui-polish.md) uses a quiet neutral rail and retains collapse, shown/total counts and direct targets. Project, Settings and conversation section headings share max(16px, 1em) above and 4px below, without stacking old padding/dividers; task-row spacing stays unchanged. These target refinements require separate implementation and native proof (R12).

Use a short neutral left rail and bold project name to distinguish groups from task rows. Retain the geometry and independent targets from [compact band A](https://github.com/taskqorg/taskq/blob/96064118e637ea8d7347edc4ca50c5c5da186757/docs/design/evidence/1805/README.md), replacing its tinted fill with the rail. Order: separate collapse arrow on the left, name and counts, then adjacent Board/Folder targets on the right. Counts share the name line when space allows, otherwise wrap beneath it. Long names wrap without clipping or separating the actions. Preserve existing palette, icons and task-row geometry.

- Show both values compactly: `2 / 4`, also `2 / 2`. Shown counts deduplicated open tasks matching the current project, state and search filters before collapse; total counts all deduplicated open tasks in that project before those filters. Matching globally pinned tasks count once; a matching pin remains in the separate Pinned section. Append `(?N)` in the panel's question color for nonzero open questions (`ask`) across the board, including filtered or pinned tasks; omit it at zero. Keep full count meanings in the tooltip and accessible name. Reuse loaded panel data; no extra board reads.
- Missing, stale or partial project data shows `?` for unproved values, with the reason in the count's tooltip and accessible name; never zero. Known groups with no matching tasks stay omitted. Keep partial/unavailable facts in the [errors list](#errors-and-service-notices).
- Only the arrow toggles row visibility; name, Board and Folder retain their independent destinations. Collapse changes neither count nor filter and writes no queue state. Collapsed headers also show each nonzero status count across all deduplicated open board tasks, using the existing status order, icons and colors (failures use the error color). Each count keeps its full status name in a tooltip and accessible name; wrap counts at narrow widths. Unproved status/question counts stay unknown with the project reason, never zero. New groups start expanded. Store collapsed canonical board identities (provider, server, repository) in the existing M8 navigation document's dashboard model, shared by duplicate registrations. Refresh, filtering, view changes and reopen retain the choice; removed projects lose it.
- Tab reaches arrow, name, Board and Folder separately; Enter/Space activate buttons. The named collapse control exposes `aria-expanded` and `aria-controls`; collapsed rows leave the tab order. Retain visible keyboard focus and 32/44 px targets. Native rendering and assistive-technology qualification follow R12 and §7.8.

Compact rows (taskq.md Product): empty search is an icon and an active query stays visible; batch Held admission shares the counter area with an explicit scope, never a dedicated row; project headings follow the shortcut row above ([D-A12–D-A15](operator-ui.md)). Growing input: conversation input grows with no manual resize or inner text scroll; history keeps its reading anchor and scrolling up pauses following until the explicit latest-message action ([AI workspace](ai-workspace.md#workspace-dialogue-d1d8)).

- One tray per host reads the registered-project list. Never discover projects by scanning folders.
- Observation does not grant admission. Focus plus host constraints determines eligibility; counters still describe observed boards.
- Deduplicate tasks by provider, server, repository and issue. A project shortcut is not a scope filter.
- Home shows pinned and recent projects; All opens a searchable complete list. Initial visible count and recent ranking follow the proposals in adaptive-menu.md.
- Preserve pin order. Do not reorder under pointer or keyboard focus. Pause/empty state does not remove registration or pins.
- Attention includes every observed project, including those outside the shortlist. Missing/stale data remains unknown.

Counter scope, all states and Held drafts with Start: [K-A1–K-A6](task-counters.md). Aggregate strip sits immediately above Projects; project strips use the same order.

## M3 Navigation and tasks

The complete task-counter filter row sticks immediately below the pinned host strip (M3.1). The host strip starts flush at its pinned position, without an initial top gap or pre-stick movement. Both pinned rows stay opaque in M1.2; details and tasks use the existing content scroll. All task-counter lines, including wrapped Held/Start, precede the filter/search/settings row. The filter row starts with one List grouping toggle, then the current-section dropdown and its chevron, the previous-section button, Search and view settings. Pressed means flat list; unpressed means grouped by project. Its accessible name and tooltip state the current mode and next action. The dropdown lists every available section/view and marks the current one. Selecting a distinct section remembers the departed section; selecting the current one preserves that memory. Previous swaps the pair repeatedly. Before a distinct second selection the previous button is omitted; previous is window-local, with no restart persistence. Grouping toggles the existing combined/project setting, preserving section, state/project filters and query and synchronizing the View editor.


M3.2 Owner rule (#1709): no horizontal scrolling anywhere in the menu, View window or Settings. Rows wrap; view picks and state counters keep each chip's icon and count fully visible, without an edge mask or clipping. The #1701 header height budget yields to wrapping.

M3.3 Task-list filters use the counter strip: all eight state icons, icon-only zeros, explicit unknowns, OR toggles and aria-pressed. Sigma (U+03A3) plus the scoped deduplicated open total comes first and clears state filtering only, preserving projects/search. Its accessible name says All tasks and the exact count or unknown. Project summaries retain nonzero counters. Filtering changes no queue/admission state.

- M3.4 Native OS chrome owns title, drag and Close on macOS and Windows, including Menu, Settings and View. The fixed content toolbar keeps Home, contextual Board and Emergency stop, one Conversations navigation button, notification-type icon/count counters, Settings, Keep menu open and Always on top as applicable. New and Needs me retain their M14 meanings and independent counts. No duplicate app Close or communication entry. Home reserves no Tasks-heading row. Contextual Back, parent breadcrumbs and current heading share aligned type, spacing and baseline; title wraps at narrow widths. Settings remains a separate window; footer/version and corner grip remain.
- M3.5 Moderate rounded corners on every side except the edge attached to the menu bar or taskbar in popover mode; pinned windows round every corner.
- M3.6 Home: pinned host/agent strip, view and counter controls, then current tasks and project groups. Questions open their exact chronological position in Conversations. No main-screen Tasks heading, composer or inline task detail surface.
- M3.7 Attention contains decisions, failed tasks and actionable service/admission problems under [A2](adaptive-menu.md#a2-explain-the-queue). Ordinary waits remain neutral.
- M3.8 Project header retains Back, linked parent Projects, current project heading, Board and compact nonzero counters. Breadcrumb and heading share baseline, line-height and readable hierarchy; wrap without clipping or repeating the current name.
- M3.9 Task rows use one shared renderer and appearance in the main list and Conversations sidebar. One whole-row accessible button opens the exact board/task in Conversations; no nested board/session/action/detail targets. Retain state/provider/project/number, full wrapped title, recorded runtime/remote marks, stage and concise warnings. The [numbers-only timing group](project-blocks.md) shows total since first worker ownership, actual subsequent state-change count and current-state elapsed. Unknown ages/metadata stay explicit. Pin, board/session/PR links, actions and full diagnostics move to the selected Conversations information area.
- M3.10 Task details/actions live only in the [unified reader](tray-workspace.md#unified-task-and-session-reader): compact sticky identity/status/action/link/question-jump header, then non-sticky full metadata, then one chronological history. Keep revision-fenced controls, questions, answers and receipts. View conditions/Saved views retain their existing editor.
- M3.11 Tasks list order (#1523): global task pins ([P-W1](menu-product.md#windows-and-task-pins)) come first, once each, in insertion order; other rows sort in the counter-strip order (Doing, Review, Ask, Failed, Ready, Blocked, Later, Held), then priority (`p0` first), then number, then canonical forge/server/repository identity (O2). The strip and the rows read the one `states` list of [project blocks](project-blocks.md).
- M3.12 Counter chips: one shared chip style. A zero chip centers its icon; a chip with a count centers icon and number as one group with equal padding. No per-chip offsets.
- M3.13 Task row text (#1524): a tray row and a `taskq status` task row show only the owner texts named here; raw execution, fence and wait diagnostics stay in `status --json` (`execution_wait`, `executions`) and the service log. Rows carry `wait`, one of: `Waiting for a free slot` (admission or load gate), `Waiting for GUI access` (GUI eligibility), `Waiting for the agent to accept input` (input acceptance), `Waiting for your answer`, `Waiting for the previous run to end` (termination, cancellation, worker-to-reviewer handover), `Run state unknown; see service log` (missing, stale or unknown observation), `Agent exited; input recovery required` (current pending input behind an unaccepted Exit). Session text: `Session unavailable` or `Session starting or unavailable`, title `Codex session opens only on <host>`; status session cells `<host>: starting`, `<host>: running`, `<host>: Remote Control unknown`. Other row text is the state name, age, project, host, runtime, the R6 `blocked (...)` dependency reasons and the separate [execution health warning](status-report.md#task-execution-health); its fixed slot exposes the diagnostic on keyboard focus or hover.
- M3.14 The entire task row opens its exact Conversations task once. Board, native-session and known PR links remain independent named controls in its detail page, never nested in the list target.
- M3.15 Codex/Claude marks identify the recorded runtime; codex-app uses Codex identity. Missing permalink: unavailable with reason. A Claude session link is account-scoped and shows on every host; a Codex thread link opens only on its owning host, elsewhere unavailable with a reason (Codex Remote cannot attach to live CLI threads). A non-local row shows the host name and a remote mark before the runtime mark, no remote text.
- M3.16 Conversations task detail preserves question, failure, work, ready, dependency and later access, including links/actions formerly in list rows. No list-row expansion remains.
- M3.17 An Ask row uses the shared task template and opens its exact current question in the chronological Conversations history. The menu has no independent question reader, inline details or competing row actions.
- M3.18 Answer cards mount only for `ask`, never for stored cards on doing/review/ready/later/blocked rows. An ask with no parsed choices shows its original text, a requeue reason field, Later and Close. One Close without publishing action dispatches canonical `later` and `close` with readback after each; no second step after failed/unknown parking. When the task leaves ask, its operation receipt leaves the card: accepted shows a 6 s overlay toast (×) that never moves rows and joins a collapsed Attention group `Answered` (newest 20, × each); unknown, rejected and stale stay as Attention rows, also after their project's task rows, with ×, unknown with the same-ID delivery check. Nothing stacks above Home; partial close names the parked task and board recovery. Target (TW2): the card opens in the workspace task-answer context, not in the menu; the same rules apply there.
- M3.19 Back restores list/scroll position. Service, log, folder and admission controls always identify project/host.
- M3.20 Current Agents is host-level, read-only: version/account or not installed/not logged in/unknown, plus probe age. Include disabled runtimes such as codex-app where reported.
- M3.21 Agents row (shipped): one per § 5 `agents` runtime, `<runtime> <version> · <account>` or `· not logged in` / `· not installed` / `· login unknown`, with the age of the freshest `status.json` probe; none: unknown.
- M3.22 Shipped popover (§ 4 Tray): Home, Projects, Attention and Settings, with Back restoring the previous route and scroll. Home shows scope, questions and failed of all projects (one task per board and issue), project rows that open in place with issue links and the board, errors and stale `status.json` in the header errors list, apart from failed tasks, and `Paused N of M` with the paused projects.
- M3.23 Project page `Capacity` (this host only, other hosts unknown): per runtime `used/limit`, `Load gate: Off|Ready|Waiting: CPU|Waiting: memory|Waiting: recovery|Unknown: <reason>`; after the § 4 `Edit limits` confirm it shows `Saved; waiting for service` until § 4 `Applied`.

### Running agents

M3.24 Home shows observed executing workers/reviewers across registered boards, deduplicated by board/task/execution, never inferred from task state. Host totals precede overall runtime totals; Codex app joins Codex. Codex, Claude and Antigravity each use their shared recognizable runtime icon plus count, with no compact Antigravity text label. Partial nonzero confirmed subtotals show ≥N; no confirmed subtotal shows ?. Remove standalone compact Partial/Unknown text while retaining accessible explanations naming hosts, age and cause, and expanded sourced details. Exact zero requires complete fresh observations. Conflicts count toward neither confirmed host subtotal. Execution-only hosts still contribute to total uncertainty; sign-in and capacity stay separate.

M3.1 The Home host strip and filter row stay pinned at the top while the list scrolls, with opaque fills (M1.2); no task row draws over them. Expanded host details scroll with the list. The strip uses compact host labels followed by each host's total, then overall runtime totals (M3.24). A label is the first three letters, extended until distinct; full name, counts and reasons remain in its tooltip and accessible name. Keep fresh idle 0, aged Last known values during a failed read, unknown `?` with sourced age when no prior observation exists, and justified partial `≥N` with a group-level explanation. Local sign-in warnings remain separate from process counts. Strip hosts have an open heartbeat; retirement follows [taskq.md section 4](../../taskq.md#4-service). Execution-only hosts stay in detail and overall uncertainty. The service reports its own host at its just-written heartbeat time. This UI30 target preserves the collector and requires implementation/qualification.

M3.25 Local capacity remains a separate observation: show occupied slots and held/requested reservations not matched to a running attempt, with its own age. Those reservations include unconfirmed executions, not just pending starts. Remote capacity is unknown without a local observation. Display Doing/Review task counts for reconciliation; neither task-state totals nor host-wide occupied slots must equal running counts. Host capacity can include other projects. Read-only rendering changes no admission policy. Acceptance: two hosts, a stale host, pending reservations, duplicate boards and incomplete data; both themes require owner approval before merge (#1349).

M3.26 A refresh publishes one complete menu view. The menu never shows content that a later update in the same refresh removes. Snapshot adoption, pending-operation projection, navigation restoration and deferred presentation must settle before publication. Missing, stale and partial data remain explicit under R12.

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

- Default 24 hours; presets 5 min/hour/day/7 days/30 days/custom. Custom start < end, explicit timezone and half-open [start,end); persist choice.
- Exactly six equal bins; presets step epoch-aligned whole bins of period / 6, newest holding now. Custom bounds stay exact. Deduplicate event identity/kind per board.
- One Rust SVG chart group supplies this panel and Summary under [R-UI4/R-UI5](rendering-contract.md#r-ui4-cadence-data), [I1–I6](icon-summary.md) and [approved review](https://github.com/taskqorg/taskq/blob/96064118e637ea8d7347edc4ca50c5c5da186757/docs/reviews/visual-decisions.md). Stack Completed, New, Ask and Failed bottom-up on one linear event scale (I3/I4). Known zero has no tick; unknown coverage is hatched. No detached failure cap or ongoing underline. Blocked remains exact detail.
- Select each bin by click or keyboard. Detail stays in this panel and names its [start,end), timezone, completed/new/ask/failed/blocked counts, per-series coverage, ongoing time and incomplete/missing source. Selection survives refresh by time identity and changes no scope, period or task state.
- Legend totals are event counts, separate from current counters; missing history shows observed lower bounds or unknown, never an exact zero or a complete aggregate. Collection start does not prove uninterrupted collection.

## M7 AI entry

- Target (TW4): the composer lives in the workspace and names the runtime (Claude or Codex, owner's pick when both exist). Current: Ask AI composer: project picker (registered projects), input, History/commands and Send. Send opens one new visible session in that project with the text as brief ([AI requests](ai-workspace.md#contract-detail)); no operator turn, conversation or confirm step.
- Under the composer the newest request shows `starting`, `launched` with `Open session`, or its no-start reason and next step. Closing the menu cancels nothing.
- History: last 20 distinct sent nonempty texts, newest first; selecting fills only. Clear affects history only. Suggested commands share the history control.
- An AI request is no task: its failure is not a failed-task count.

## M8 Implementation boundary

WS11 first slice (#976, accepted split): native Home / Projects / Attention / Settings and its twelve destinations reuse the existing snapshot and action bridge. Back restores the calling route, disclosure state and scroll; refresh keeps the current route. Existing Balancing and Ask AI remain accessible; no new backend authority. Appearance exposes the currently supported view/font controls and reports the system theme.

Settings keeps all twelve destinations. Root widgets use short scope/count/exception summaries; complete project readback stays in detail. Appearance uses one active transparency pane; Notifications uses compact event/mode rows, one shared legend and project overrides. Hosts retain comparable runtime counts and freshness. Agents lists client versions/accounts once and compares project/runtime access without repeating versions. Projects keeps compact overview/forge/folder shortcuts with intake intent/readback in detail. Delivery explains each preset, with revision secondary. Services shows the host process once, project exceptions first and complete observations/actions on demand. About keeps exact installed/running/available versions and update/rollback review. No presentation changes operational policy or authority.

### Shared view state (#1439)

`src/tray-view-state.js` owns navigation state for every tray route. Routes declare
valid identities and parents; common keys apply automatically. Views never access
storage. The module captures route (`page`), selected state filters (`taskFilter`,
OR; an empty array means all tasks), selected project (`aiRoot`), Balancing scope
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
to Projects; an unknown route to Home. Drop a removed selected project or Balancing
scope. Missing groups keep their default; scroll clamps to the available content.
A bad/unsupported navigation schema defaults to Home without resetting other
settings. Snapshot capture explicitly overrides navigation and does not save it.
Save failure is shown as Settings unknown; it never reports persistence as applied.

### Shared pending operations (#1819)

Owner decision on #1819 (option 1, pending overlay without a queue). The tray keeps
one native pending-operation store in memory, keyed by project root and operation id.
Every answer, steer and task control (`taskq.task-controls/1`) enters it when the
bridge accepts the request. Each change to the store goes to every open window
(menu, Conversations, request windows, Settings, View) at once, never on the 2 s
tick; a window opened later receives it with its first snapshot.

- One channel (#1918): the tray applies the store to the projects it reads before it
  builds anything else, so the task list, project dots, counts, Messages badge, tray
  icon, notifications and Conversations all read that one snapshot and never
  disagree. No window keeps an optimistic overlay of its own.
- Phases: `sending` (the CLI write runs; the row keeps the board's state and shows
  `Sending`), `accepted` (the receipt read the write back by its op id; the readback
  state, revision and answer apply to the row), `failed` (not sent, unknown or
  refused; a Retry button resends the same operation once on click), `mismatch` (a
  refused or stale write: the real state and a short reason for 30 s; it never asks to
  confirm again). Nothing shows a state the board has not read back.
- Confirmation is by op id, never by revision compare: § 2 revision drops after an
  answer (the ask entry id gives way to the spawn id), so a revision compare would take
  the pre-answer `status.json` for confirmation. A snapshot row lists the op ids of
  its recent log entries (`ops`); an accepted operation holds until a row carries its
  op id, or the task leaves the snapshot, and then 60 s more, so a lagging board read
  never shows the question open again. A stale snapshot never settles it.
- An accepted write appends its task to `<root>/.taskq/reread` and pokes the service;
  the next pass drops that task's cached comments and skips the 304 shortcut, so it
  rereads that task.
- No outbox and no replay: a failed answer keeps its draft with one Send button;
  nothing resends after a reconnect or restart. The store is memory only.
- While any window is visible and focused the tray touches `<install>/menu-open`
  every tick; the service then sleeps at most 15 s (§ 4 step 8). A window in the
  background keeps the normal pass, so it can show the stale line below. Board reads
  reuse ETags, so an unchanged board costs no rate-limit requests.
- Stale data: when a project snapshot is older than two poll intervals or missing,
  every window shows one line `Data from HH:MM` (the oldest observation time).
- Telemetry: CoreQ operation `tray.sync` records each write's latency from request to
  op-id confirmation and its outcome (`observed`, `failed`, `mismatch`); no second
  collector.

Deferred follow-up contracts (not acceptance claims for this slice):
- Selective recovery: E-A1–E-A9, including durable hold, owned-process identity, unknown-liveness refusal and selected fenced restart with readback. Keep ordinary pause distinct.

F5 for this slice covers the native navigation/settings ledger and preserved existing actions ([evidence](https://github.com/taskqorg/taskq/blob/96064118e637ea8d7347edc4ca50c5c5da186757/docs/design/evidence/ws11/README.md)). Full AI/recovery rows remain Not tested; no disabled placeholders count as their completion.


- Installed/source inventory: [current-menu-audit.md](https://github.com/taskqorg/taskq/blob/96064118e637ea8d7347edc4ca50c5c5da186757/docs/design/current-menu-audit.md). Historical phase numbers are not evidence that features are absent.
- Preserve all existing functions while replacing layout. Shared tray aggregates current per-project data; service ownership follows the current runtime contract.
- Add project is implemented in the current contract; new AI-window placement changes its presentation, not setup safeguards.
- Old-project marker: taskq.json with failing selftest, accepted in #766. Diagnose before any cutover; do not overwrite existing configuration.
- Native accessibility and rendering require platform evidence; HTML tests alone are insufficient.

Native sizing, scroll ownership and cadence rendering follow [R-UI1–R-UI6](rendering-contract.md).

`taskq tray --snapshot` routes: `home`, `actions`, `project`, a Settings or top-level route, `icon` (through the renderer: both themes, run and pause, named by size), and `scroll`: Home with each bounded AI block at its start, middle and end, captured only when named.

## M9 Settings and maintenance

- Categories: Appearance, Activity history, Agents, Services and About. Balancing remains a direct entry, not a second editor.
- Appearance contains view/font; Activity history contains M6 presets/custom range; Agents retains host probes; proposed permission controls follow [P1–P9](worker-permissions.md), pending authority/enforcement approval.
- Services lists targets with freshness, incidents, log/folder, restart, stop with its native restart condition, and nested uninstall confirmation. Retain Resources access to the existing external monitor and incident consent controls. No repeated service sections on home.
- Stale status does not prove a dead process. Known logs remain accessible during failure; unknown log paths are explained.
- About shows tray/service versions and host. Update now appears only for a verified newer version; preserve progress, readback, failure and rollback visibility.
- Update writes the existing service request and follows §8. Do not claim completion from a button click.
- `<install>/tray.json` holds shared navigation state (M8), view, text size 14–20, period, notification modes, cursors and open-task titles, Pin, Top, the chosen logical window size and its last position per display (off-screen: at the icon).
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

<a id="m12-shared-work-mode"></a>

## M12 Shared Balancing

The visible name is Balancing; existing `work` routes and JSON keys remain compatible. Four sections appear in order: Task routing (soft preferences and strict pins), Runtime availability (account quotas/reset/probes and factual blockers), Host balancing (host limits and load gate), Project policies (focus and lower-only ceilings). Opening changes nothing.

- Settings Balancing shows current host admission, project exceptions, saved intake/readback and direct admission/host-limit reviews. Recovery shows the current hold and service observation plus direct selected-recovery review. Neither is a launch-only page. Entering applies nothing; operation confirmations and unsupported/refusal reasons remain in the existing workspace. Back restores the invoking Settings context.
- One editor from home/settings/project; project entry preselects that project. Open it in the existing workspace (approved Workspace review), preserving menu navigation and drafts. Recovery uses the same workspace route.
- Named host; host limits and load guard affect all its registered projects. Project focus/filter/ceiling edits name their own selected projects and review; that selection never narrows a host edit.
- Selection defines project change targets, not registered observations. Unchecking leaves that project's focus/ceilings unchanged; host edits separately name their wider effect.
- Preserve runtime-specific host limits, including codex-app, and separately named lowering project ceilings. Disable unselected project fields; unknown values follow [unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy).
- Different current focus displays Mixed. Opening does not overwrite it. Change only explicitly edited fields.
- Review exact project/host old→new changes; current attempts continue. Cancel preserves settings; readback is per project, including partial failure.
- Preserve CPU/memory gate controls and saved-vs-applied distinction from the current UI. Gate semantics remain in taskq.md.
- Native tray (#954): Balancing exposes one host editor for maximum, runtime limits and load gate, without project-selection controls that could imply a narrower effect. Project entry retains that project's observation context. Fixed remains legacy-only. Use the [shared resource controls](shared-resource-controls.md#shared-component-930); project focus/filter/ceiling work remains a separate target until its contracts and backends are qualified.

## M13 Pinned window

- The menu toolbar carries icon-only Keep open and Top controls with distinct accessible names. Native OS Close hides the window; no duplicate app Close. Pin orientation distinguishes pinned from unpinned; Top uses overlapping squares. Unpinned (default): the popover closes on blur, as today.
- Pinned: the same content stays open as a normal app window in Alt+Tab where supported (macOS: TaskQ is always in Cmd+Tab, M15; unsupported behavior requires an owner decision), with the same width rules; it moves anywhere by dragging its header, does not close on blur or on a tray icon click (the click raises it).
- The window snaps to the screen work-area edges where the OS provides it (Windows Snap); on macOS it keeps within the work area and remembers the last position per display.
- Unpin returns to the popover at the tray icon. Pinned state and position persist in `<install>/tray.json`; a missing or off-screen position falls back to the tray icon.
- Top independently keeps the window above normal windows (default on); with Top off it can go behind others and a tray click raises it. Pin and Top persist independently.
- The ordinary native OS titlebar has its normal opaque background and integrated frame in Background-only opacity (Whole-window alpha applies to the whole native window, titlebar included), stays outside content scroll and owns dragging; controls act on the first click on macOS in every TaskQ window, including Menu, Conversations (main and detached), Settings and Views, even when another app is frontmost.
- Window growth offers direct Auto / Down / Up / Fixed choices with immediately animated miniatures (static under reduced motion). Auto uses actual tray placement/opening direction; Down keeps top, Up keeps bottom, Fixed keeps both dimensions. Clamp to the anchor monitor work area, accounting for the native frame exactly once. Content fitting converges after wrapping, content and titlebar changes without repeated rest-time resizing; intentional user dimensions remain authoritative. Preserve existing explicit top/bottom/fixed settings; new windows default Auto. Pin/drag preserve growth overrides. First actual native window move pins; Close is always reachable.
- A fixed footer remains outside content scroll; it shows the version only (MP15), no host/live/status prose. Its integrated bottom-right resize grip supports pointer and arrow keys without square-button chrome and changes both dimensions; the chosen logical size persists across reopen and restart. Before a manual resize, fit content at the current font and DPI up to the monitor work area: the fit height is the window's measured layout (header, chips, summary, list, footer and any added panel) with the scroll area at its full content height, never per-panel pixel constants; scroll only when content exceeds available space.

## M14 Notifications

### Errors and service notices

Tray, Settings and Conversations have one steady, non-animated errors icon in the shared header, including at zero. Its count is the number of distinct current causes. Opening it shows one scrollable list outside page flow; neither arriving errors nor opening the list grows the page. A cause shared by projects has one row naming the project count and an expandable project list. A board rate-limit cause uses board provider/server and reset time, not the operation or project prefix; independent causes stay separate. Unknown reset stays explicit. Recovery removes the row on the next successful observation, without acknowledgement.

Failed reads preserve the last known task, host and conversation rows with their observation age. Historical counts say Last known; they never claim current execution or health. No prior observation means unknown. Freshness, permission, revision and receipt checks still gate writes (R6/R12). One compact data-age label stays beside the affected values; repeated diagnostics live only in the errors list. Questions, decisions, consent disclosures and operation-specific validation/readback stay with their existing action or Needs me, never in the errors list. [Surface review](errors-surfaces.md) defines that boundary.

### Panel and attention

- One header entrance uses the bundled `messages-square` glyph and opens the common Conversations centre. New in the top task-counter bar counts unread canonical threads; Needs me counts the union of current unanswered questions and due undecided reports. These sets may overlap and are never added. Each top-bar type counter has its own semantic glyph and accessible exact count; cap visual numbers at `99+`. Unknown/partial sources name the reason, never a false zero.
- Without saved reading context, prioritize Needs me when nonempty, otherwise New/All. A saved context or explicit deep link wins; new arrivals never steal selection or reorder a visible row under the pointer. `notifications` remains a compatibility route; `notification-settings` owns OS delivery. No competing bell or independent menu reader remains. Read/clear never removes unanswered questions or required decisions. Home retains unresolved questions and faults.
- Use collected results (one per exact result SHA) and retained project events, newest first; a collected result replaces its matching close-event row. Deduplicate identical board/event identities across project registrations. Each row names kind, project, task and Read/Unread; opening it marks only that event read and opens the same Conversations task/result with its exact event or SHA. Opening New or expanding a group acknowledges nothing. Missing history or cached content stays explicit; no board read is needed to navigate.
- Events of the same project, kind and numbered title series (`Cadence v4 1/10` … `10/10`) within ten minutes of the previous series member form one collapsed expandable row. Match the title prefix and denominator; distinct prefixes, projects, kinds, repeated sequence numbers and longer quiet gaps start another group. Show member and unread counts; members remain individually readable and openable in newest-first order. Unnumbered events remain individual rows; interleaved unrelated events do not split a series.
- `Mark all read` marks the displayed snapshot, including collapsed members; later arrivals remain unread. Completed-result reads retain the existing SHA-bound board marker/readback; other event read markers are host-local. `Clear read` hides only confirmed read notifications on this host, never drops a report, accepts a result, answers a question, deletes event history or changes a task. Unread events survive. Read/clear state survives tray restart in `tray.json`; failed persistence/readback stays explicit and retains the prior state. Clearing can be repeated safely.
- Native snapshot routes: `notifications` for the compatibility entry; `notification-settings` for OS delivery; opt-in fixture cases `notifications-home`, `notifications-panel`, `notifications-expanded`, `notifications-result`.
- This uses the [comparison and owner walkthrough](https://github.com/taskqorg/taskq/blob/96064118e637ea8d7347edc4ca50c5c5da186757/docs/design/evidence/1964/README.md); M14 changes presentation and notification metadata only, not §2 lifecycle or OS-banner delivery. Unaffected geometry and glyphs keep the pinned visual reference.

### OS delivery

- The tray posts native OS notifications (macOS Notification Center, Windows toast) for new board events read from each project's `events` (§ 4 Tray). A closed task keeps its title in its banner and card.
- Event kinds: `ask` (a question waits for the owner), `close`, `failed` and `gone`. Settings > Notifications (`notification-settings`) sets OS delivery for each kind to Off (no OS delivery), Silent (no OS banner), Banner or Banner with sound. Panel retention/read state is independent of these delivery modes. Defaults: `ask` Banner with sound, `failed` and `gone` Banner, `close` Off.
- A project may override the host setting per kind (Inherit by default); the project page shows the effective value.
- A notification names the project, the task title and the kind; a click opens the exact task/event in Conversations; only a current question exposes the existing answer card. Reading historical questions grants no answer authority.
- Once per event (§ 4 Tray): events at or before a project's cursor (the last notified event id), or older than the first tray start with notifications on, never notify. More than 3 events within 10 s become one summary notification per project.
- The first enable asks the OS for permission on macOS; Windows reads the OS notification setting without a consent prompt. While the prompt is open Settings shows `Asked`; a failed request shows its OS error, never `Not asked`. Denied or revoked: Settings shows `Blocked by the OS` with a link to the OS setting, and nothing falls back to another channel. `Send test` posts one sample of the selected kind.
- An unknown `tray.json` value reads as the default and keeps the file.
- Windows toasts use AppUserModelID `org.taskq.tray` on a TaskQ-owned Start Menu shortcut to the stable tray executable, whose toast activator CLSID names the running tray's COM activator, so a click on a banner or in Notification Center reaches it; OS settings are read through the notifier (denied: Blocked, no setting recorded before the first toast: the OS default, allowed, other errors: Unknown). A failed authorization request is logged in `tray.log`.
- Packaging: Windows runs the tray as `<install>/taskq.exe` (the launcher, M15), so the notification-area pin follows the exe path across updates. macOS notifications show the TaskQ app icon, packaged in `<install>/TaskQ.app` (the default install: `~/Applications/TaskQ.app`, M15) (bundle `org.taskq.tray`, signed as taskq.md § 8 step 5), which notifications need, started through LaunchServices (`open`; the bundle with no args is the tray of its parent folder and appends its stderr to `tray.log`): a bundle exec'd by the service is refused authorization.

## M15 App lifecycle

- TaskQ is one normal app per host: one tray across all install folders, held by an OS temp-dir lock for the tray's life. A tray that cannot take the lock exits; a tray of a removed install exits on its next tick.
- Users see one app named TaskQ, never an app plus a service: app text (app menu, tray menu, tray tooltip) says TaskQ and never says service. Settings > Services keeps its operator terms.
- macOS: TaskQ.app is always a regular app with a Dock icon and a Cmd+Tab entry (no `LSUIElement`); the menu bar icon stays. Its app menu holds About TaskQ, Hide TaskQ (Cmd+H), Hide Others, Show All, Restart TaskQ and Quit TaskQ (Cmd+Q). Windows: the tray menu (right click) holds Open, Restart TaskQ and Quit TaskQ.
- Open: macOS `~/Applications/TaskQ.app` (the default install's bundle; Finder, Spotlight, Dock), Windows Start menu `TaskQ` (`TaskQ.lnk` to `<default install>/taskq.exe`, no arguments), its taskbar pin and a double-click on `taskq.exe`. On Windows `taskq.exe` with no arguments opens TaskQ when its console is its own (Explorer, Start menu, pin): it drops that console, so no console window stays; in a terminal it prints help as before, and every other command keeps its terminal output. Open starts a stopped service for the registered projects and shows the tray. A second Open raises the running app's window (macOS reopen event or Dock click, Windows a raise request the running tray takes on its next tick). The tray menu's Open raises it too.
- Closing or hiding the window (close button, Cmd+H, blur of the popover) only hides it: work keeps running and the tray icon stays.
- Only the tray of the default install (the OS default folder, not a `TASKQ_INSTALL` override) writes the Start menu `TaskQ.lnk` and `~/Applications/TaskQ.app`; on start it repairs a `TaskQ.lnk` of its own identity whose target differs. A tray of another install (tests, temp installs) never writes them. The Dock item and the taskbar pin keep their path across updates: each update replaces the binary behind the same `TaskQ.app` and `<install>/taskq.exe`. No `taskq-tray.exe` copy exists; an update sweep removes an old one once no tray runs it.
- Quit TaskQ: the tray menu (right click) and, on macOS, the app menu and Cmd+Q. The tray menu names the active work Quit waits for (doing and review tasks). Quit drain-stops the service: current work finishes, nothing new starts; a task whose worker asked or exited resumes after the next start (taskq.md § 4). On macOS the drain keeps a Quit hold (Stop mode `quit`); the unit keeps OS autostart, so the next logon starts the service and its tray, which reopens TaskQ as Open does. The tray exits once the service has stopped and removes its `<install>/tray` heartbeat.
- A drain stop (Quit, Restart TaskQ) stops admission only. Every registered project keeps refreshing `<root>/.taskq/status.json` at its poll interval: a project with nothing left to drain only reads its board and beats, without claims or board writes, so tray counters and host ages stay known until the next generation starts (#1766). `taskq profile health` prints each project's status age and marks one older than two poll intervals stale.
- Reopen restores both: Open clears only a Quit hold whose service generation has gone (an older operator hold returns), then starts the service. Open never clears a reviewed Stop hold (drain or full): the service stays stopped until reviewed recovery (#1285). `service release` still refuses every Stop hold.
- Restart TaskQ: the tray menu and the macOS app menu. It drain-stops as Quit does; once the service has stopped, the tray releases its host lock, removes its heartbeat, starts a new tray with Open (macOS through LaunchServices for the bundle) and exits. Open then starts a new service generation.
- A refused Quit or Restart (service observation stale, another lifecycle operation pending) keeps the tray and service running and shows the reason in the tray menu and tooltip.
- Tests that start a tray from a temp install stop it before they end: the lock refuses a second tray, and removing the temp install ends a tray it started.

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

General principles change per [shared-rule improvement](https://github.com/taskqorg/specq/blob/main/engineering/spec-first.md#improve-shared-rules); TaskQ behavior changes here under R13.
| M-A22 | Each kind follows its host or project setting; an event notifies once across tray restarts; a burst becomes one summary; a click opens the card; OS denial shows Blocked and posts nothing. |
| M-A24 | One Conversations entrance shows independent canonical-thread New/Needs me counts, capped visually at 99+ with exact accessible counts; incomplete sources are explicit; questions remain on Home. |
| M-A25 | Series expand to every event; open reaches the exact card; mark all read preserves new arrivals; clear read preserves unread events, questions and task state across restart. |
| M-A26 | Home has no finished-result/event feed; Notifications and delivery settings remain distinct and usable in both themes at 320 px without horizontal scroll. |
| M-A23 | On macOS and Windows: launcher Open starts service and tray; a second Open raises the window; Quit names active work, and readback shows the service stopped and no tray pid; reopen restores both; a second tray across install folders exits. Restart reads back a new service generation and one tray pid; a non-default install's tray leaves `TaskQ.lnk` untouched; the Dock item and taskbar pin open TaskQ after a release update. |

## Contract detail

Moved verbatim from `taskq.md` (#1179) to keep the contract under its R8 size bound. `taskq.md` keeps the decision and links here; this text keeps the same authority (R8, R13).

- Tray app (`taskq.md` § 4): one normal TaskQ app per host: Open starts all; hide/close keeps work; Quit drain-stops; Restart: Quit+Open; app text never says service. Service starts it if `<install>/tray` pid >30 s old. Reads `<install>/projects`, each `<root>/.taskq/status.json` and `events`; no board/token. View state/settings: `<install>/tray.json` (M8/UI02), no task truth. Writes: target/effect first; success only on readback. Menu/detail/views: [task workspace](task-workspace.md) `taskq.task-workspace/1` (`taskq.receive(state)` in, ipc JSON out), same build: tray, DashQ.
- Tray (macOS, Windows; layout, routes and platform packaging: [menu specification](menu.md) M1–M15): one per host. Any installed service starts `releases/<current>/taskq tray` (macOS through `TaskQ.app`, Windows as `<install>/taskq.exe tray`) when the heartbeat `<install>/tray` (the tray's pid) is older than 30 s; a tray that finds another fresh pid there exits, as does a tray on another version than `<install>/current`. It lists the projects of `<install>/projects` and reads each `<root>/.taskq/status.json` and `events`: no board, no token. Icon: the compact q with ▶ when a project admits, Ⅱ when none does, gray when all are paused; macOS may instead show Summary  ([square summary](icon-summary.md); Windows availability: M1), drawn by one SVG renderer (`src/icon.rs`); a saved Custom selection falls back to Summary. Writes name their target first: pause/resume per project and over all after a confirm naming each project and its effect; resume without a saved focus offers the project's labels and writes the pick; per project log, folder, restart, stop until the next logon, and uninstall after a confirm naming it. `Edit limits` writes only after a confirm naming project, host, old → new and `Running tasks continue`, and shows `Applied` only when `status.json` reads the values back. `Update now` shows when a `status.json` names an `update`: it writes that project's `<root>/.taskq/update` and reports success only when a `status.json` reads back that version. History (M6) combines `events` with the versioned first-admission projection in `status.json` (I3); both are rebuildable observations, not queue state. Agents (M3) shows the § 5 `agents` probe, read-only. Resources opens btop in its own OS terminal window, never inside the menu; missing btop is installed first with the OS package manager, unelevated (M9). Notifications (M14): native OS notifications of new `events` lines, per kind and project as M14 sets, at most once per event. Tray settings, cursors and open-task titles persist in `<install>/tray.json`. `taskq tray --snapshot DIR [--route R,..]` writes the icon PNGs and popover snapshots of the current state in both themes, then exits; an unknown route is refused (routes: M8); the capture window stays off the visible screen where the OS allows and takes no focus.
- Status (`taskq status`, § 5): `--json`: the snapshot: `schema` 1, `source` board, `observed` board time, `complete` and `reason`, `board` identity, `totals` per open state (null when incomplete: [unknown](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy)), `tasks` (labels, session, `session_url`, deps, card), `hosts`, `capabilities`. `--closed`: one board page of closed tasks (closed at or after board second T) as JSON `{schema, source, page, more, tasks: [{n, title, url, closed, outcome, result, pr, merged}]}`; outcome `success`, `not_planned` or `unknown` (§ 1); `merged` is the PR/MR merge receipt of the task's `close` entry (§ 7), else null (unknown): issue closure or a PR link is no merge evidence, so the tray shows Merged only with it.
- Snapshots: `taskq tray --snapshot DIR [--route R,..]` captures icons and popovers in both themes.
- Events: a pass appends to `<root>/.taskq/events` a line `<board time> #<N> close|failed|ask|blocked|gone <id>` per task it saw move to closed, `failed`, `ask` or `waiting` since its previous pass (a new task counts from its first state); the id is the attempt current at the move, else the task's last spawn; for `ask` and `blocked` else its last comment, else the issue number; a closed or `failed` task never spawned adds none. A `close` line ends with its [§ 7 outcome](../../taskq.md#result-and-close-verdicts) (`delivered`, `no change` or `replaced by #M (<state>)`, plus `, N items unverified`); the tray passes it as a 5th event item and the close notification reads `#N closed: <outcome> · <title>`. `gone`: per trusted `gone attempt <id>` log entry newer than the previous pass (the first pass adds none). One line per attempt and kind; lines older than 30 days drop. The first line `since <time>` is the first time covered (`events_since`); a restart keeps the file, moves while no service ran are missing. `ponytail:` no board replay. `ponytail:` files, no socket or pipe, on every OS.
- Session links (R6): Claude links use only the Remote Control URL registered by that native session; the local UUID is not a remote ID. Codex links on the owning host: `codex://threads/<id>` when `TASKQ_CLIENT=codex`, else `<pages>/open.html#codex://threads/<id>`; on another host the DashQ progress page `<origin>/projects#board-<repo>-task-<N>` of the active connection (R6 3), else the host name alone.
Owner-facing service/tray text follows R7: counts use R6 names/order; task rows are `#N <title>` (title at most 60 characters). Use `Focus: <labels|off>`, `Paused`, `Service: <running|stopped|unknown>`, `Update: <version|up to date|unknown>`; unknown/stale values per [unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy). Menu action labels at most 32 characters; summary/reason lines at most 160; navigation per the menu design and [D8](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md#principles); CLI lists wrap one project/label per line when needed.

A corrupt `status.json`, `tray.json`, `kids` or `focus` reads unknown (reason in `status.json` `focus_error`, the tray's header errors list, `service status`), admits or adopts nothing and deletes no release; a tray save first copies a corrupt `tray.json` to `tray.json.corrupt`.

### Closed pin reads (P-W1)

`status --closed --task N` reads one exact issue and its trusted log, independent of history paging or period. It returns the board close time, outcome and close verdict; an open issue is explicitly open. `--transcript` reads a bounded native transcript page on the recorded worker host, with `--before B` for earlier records. A missing, remote or unsupported transcript remains unavailable with a reason. These reads never start or resume a session. The token-free tray invokes the owner CLI for the registered root. Only the reference and first verified closure time persist in navigation; task and transcript data remain transient.
