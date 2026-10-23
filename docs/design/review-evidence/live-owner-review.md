# TaskQ #1573: live owner review

Status: owner walkthrough and full top-level route audit in progress, 2026-10-23. This report is not acceptance of either delivery. Collect owner remarks here, reproduce them in the live application, and reconcile each with the effective contract before proposing fixes.

## Baseline and authority

- Owner requests a live design/usability review and a package of corrections. No implementation, board changes, publication, service restart, permission change or live request submission is authorized by this review.
- Handoff: `taskq-design-snapshot/.claude/worktrees/unscheduled-tasks-queue-4efbc3/review-brief-1573.md`.
- Native application selected by exact path: `local-install/TaskQ.app`. Another test application shares its bundle ID; it was not used.
- Observed live footer and selected installed release: `4.6.34-dev.1792726249`. Real task rows include TaskQ and DashQ; the conversation workspace contains the existing `taskq status` request. No fixture substitutes for this evidence.
- Read-only source review: checkout `99b2ae1cb3d566eef99ee328f6ac4581fa687cb7` at the handoff worktree. Both delivery merge SHAs are ancestors. The exact source SHA of the running binary has not been established; do not equate that checkout with the native artifact without further provenance.
- The original design checkout remains dirty at `590b87e457004594b39f7105d3b505bf18630bfd`; it was not reset, updated or used as current implementation.
- Frozen design reference: `control-panels.html`, SHA256 `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`. Later owner decisions and current contracts supersede affected historical behavior.
- [O1–O3](https://github.com/taskqorg/taskq/issues/1573#issuecomment-6340259438) are verified live from the board: row=current stage/elapsed; comparator=fixed state/priority/issue/canonical repository; working changes immediate, Save updates preset, Reset restores it.
- [#1580 answer 2](https://github.com/taskqorg/taskq/issues/1580#issuecomment-6343852087) explicitly selects native-only dialogue. No in-workspace reply chain is an accepted scope reduction, not by itself an implementation omission. Reopening that product decision requires the owner.
- #1578 merged as `635f9b16a6d11e9222a20e206c6d865df9dc9930`; #1580 as `9661f2bef09673855e0a8fced224914ee64d8821`. Close/check prose is historical evidence, not the present review verdict.

## Captured live evidence

1. [Tasks and anchored details](01-live-task-details.png), [AX observation](01-live-task-details.ax.txt): real filters, state counters, titles, stage age and #1594 preview. A live refresh changed task order as a task entered Doing. No filter was changed for this capture.
2. [Conversations](02-live-conversations.png), [AX observation](02-live-conversations.ax.txt): existing launched request, Open session, independent next-request composer, list and footer. Opening Conversations was navigation only; no draft was created, edited, sent, archived or deleted.

Initial screen set: 2 examples; the route audit below extends it to 9 of at most 10 retained examples. These observations cover the current native theme and user geometry only. Passing evidence belongs in text; add images only for material findings or missing states.

## Initial findings

- P2 — [Live tasks/details](01-live-task-details.png), [task-dashboard.js:19](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/task-dashboard.js#L19) — All eight selected states expand into a three-line condition sentence before the separate counter toolbar, while the result count and All count repeat nearby. D4/D9 and MP12 task dominance are weak at this real width. Normalize the equivalent full-state selection to a compact “All states” summary and keep the detailed criteria in the existing editor; retain the useful scoped total and zero-state choices where needed. Do not change filter semantics or simply remove useful access.
- P2 — [Task details](01-live-task-details.png), [task-dashboard.js:45](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/task-dashboard.js#L45) — The ordinary task preview exposes a long attempt ID and identical revision ID on two lines ahead of actions. Move diagnostic identity into an explicit technical disclosure while retaining exact values for support and operation fencing. Current-stage age, total age and steps remain available.
- P2 — [Conversations](02-live-conversations.png), [conversations.js:116](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/conversations.js#L116) — The existing launched request is dominated by verbose launch instructions and technical identity; the native-only boundary is repeated around the next-request composer. Keep one concise truthful launch state and a clear Open session action, with technical identity and detailed explanation on demand. Verify against the exact live render producer before implementation; the inspected source does not establish the binary's exact revision.
- P2 — [Task header](01-live-task-details.png), [tray-app.js:516](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/tray-app.js#L516) — Emergency stop uses `circle-pause`, visually suggesting ordinary Pause despite different effects. Propose a distinct stop affordance with an accessible name and the existing consequence review; do not change or test operational effects during this review.
- Product decision — [Live Conversations](02-live-conversations.png), [approved scope](https://github.com/taskqorg/taskq/issues/1580#issuecomment-6343852087) — The surface named Conversations is a request/draft manager: it cannot display or continue the native dialogue. “New from draft” creates a separate request. If the owner still requires a messenger workspace, prepare an explicit revision of the native-only decision; do not call that missing bridge delivered or silently restore a historical operator.

The zero-state buttons are deliberate interactive filter choices in `project-blocks.js:26–35`; their existence alone is not a counter-correctness defect. Review their placement and density separately from data truth. The original total-age/step-count request is settled by O1 and must not be reported as a missing row feature.

## Acceptance audit: current evidence, not close verdicts

“Partial” means some source or live evidence exists but the whole acceptance line is not proved. “Not verified” is not “Missing”. A control existing in code does not establish a successful native action. No destructive or operational action is exercised merely to fill the table.

| Task / item | Current verdict | Evidence and remaining proof |
|---|---|---|
| #1578 A1 | Partial | O1–O3 and frozen reference are present; both merge SHAs belong to inspected source. Full contract/reference reconciliation and exact live artifact provenance remain to check. |
| #1578 A2 | Partial | Direct Inbox/Conversations/Settings, wrapped rows, shared-looking icons and compact host line observed. Header/filter density findings above; complete project/agent information and popover access still need review. |
| #1578 A3 | Partial | Current saved/working view, search, state counters and scoped All visible. Source has overlay/group/save/reset routes. Multi-project/grouped behavior, independent view recovery and restart have not been exercised live. |
| #1578 A4 | Partial | Real identity anchor, wrapped titles, stage age, total age/step count and operation entry points observed. Expanded ordered durations, timing truth, keyboard preview and exact action fences remain unverified. |
| #1578 A5 | Partial | Pin/Close/footer/corner grip present. Growth settings, first-drag Pin, real placement, direction retention, resizing and restart not exercised. |
| #1578 A6 | Partial / live route gaps | Top-level routes now inspected. Access is a redirect loop/placeholder; Delivery is read-only; host Service repeats per project; Work scope is Pause/Resume rather than a full predicate editor. Resource editor exists. Operational effects remain untested. |
| #1578 A7 | Partial | Live Appearance, Icon/Summary and Window routes inspected without preference changes. Later Custom removal is a contract change, not automatically a regression. State/grant effects and cross-domain persistence remain unqualified. |
| #1578 A8 | Partial | Current live macOS screens inspected; builder/reviewer evidence exists separately. Both themes, 16/20px, boundaries and native gestures/accessibility remain an open review matrix. |
| #1580 A1 | Partial, amended | Answer 2 verified; real launched request and native session link observed. Actual read/raise handoff was not invoked; no reply chain is promised under the amended scope. |
| #1580 A2 | Partial, amended | List/record/composer, New/search/Archive/History, Work sessions and Pin/Close/enlarge/detach entry points observed. Native ownership, narrow return and resize remain unverified. |
| #1580 A3 | Not verified live | Source provides creation/selection/caret/search handling. Independent drafts, focus/scroll, clear-search and restart must be checked without disturbing owner data. |
| #1580 A4 | Not verified live | Archive/Restore/Undo and permanent-delete ownership require a reviewed safe scenario. No real record was archived/deleted in this review. |
| #1580 A5 | Partial | Attach entry and explicit limits exist in source. Real select/drop/preview, immutable sent bytes, storage failure and restart not tested. PDF is explicitly unsupported. |
| #1580 A6 | Partial, amended | Live UI directs follow-up to native dialogue; source exposes same-ID reconciliation and prelaunch cancellation. These operations and uncertain/partial outcomes have not been tested here. |
| #1580 A7 | Partial, amended | Open session points to a real Claude URL; Work sessions entry exists. Source-numbered answers, native persistence and unsupported targets need separate verification. Full replies remain native by answer 2. |
| #1580 A8 | Partial | Two real screens now complement historical browser evidence. Chat-first reload, recovery, IME/accessibility, long/narrow/both themes and native handoff are not yet requalified. |

## Next owner walkthrough

1. Owner-selected remarks on the currently visible screen; reproduce, classify and attach evidence before proposing changes.
2. Task filter/preset and project grouping; task details/step history and return, preserving working context.
3. Settings and actual window behavior; inspect useful system data versus unavailable or thin routes.
4. Conversations decision and complete permitted journey; distinguish product scope, usability defects and unverified adapters.

At convergence, group confirmed fixes into a few coherent user outcomes: task dashboard/navigation, operational settings/window, and request/conversation workflow. This is a provisional grouping, not filed tasks or an approved implementation plan. PM receives the final package only after owner agreement.

## Limits

No tests, snapshots command, installation, request launch, board mutation, model/grant change or service operation ran. Repository source and contracts were read only. No screen-reader/IME, Windows/DPI, physical gestures or live operational effect is qualified. Existing screenshots contain real operational context and stay local; no upload was performed.


## Owner corrections: first review round

These are explicit owner requests in this review, not incidental prototype behavior. Record them now; amend the owning product contracts before implementation. Source and running settings remain unchanged. Original hover behavior is superseded by this owner's click-only direction; the earlier approved absence of a Closed counter does not prohibit the newly requested completed-task browser.

| Area / requirement home | Owner-requested outcome | Review disposition |
|---|---|---|
| Task details — M3 / task-controls / MP O1 | Mark the existing identity anchor as clickable. Open details only on click or explicit Enter/Space activation; focus or hover alone must not open them. Keep Close/Escape and predictable dismissal. | Selected correction. Diagnose refresh/focus and pointer-driven opening; do not offer hover as another candidate. |
| Details content — M3 / Q1 | Rework real task details, especially questions, so decision, stage/history, result and relevant actions are clear. | Inspect owner's representative task/question cases next; do not approve a generic placeholder layout. |
| Header — shared shell / F3 | Stable actions in the first row. Title/breadcrumb can use the row below when space is tight. Home always returns to the first task screen in one activation without resetting work. | Selected correction; shared menu/workspace component needs one fix. |
| Attention indicators — MP12 / messages | Visible new-item badges on Inbox and Conversations where an authoritative unread source exists. | Inbox count exists only in accessible text today; no visible badge. Native reply unread state is unavailable under native-only dialogue and must not be invented. |
| Task session access — M3 / C6 / R6 | Runtime brand icon immediately left of current-stage timing; additional remote-host mark when actually remote. Enabled verified session target opens directly; unavailable target is visibly inactive with an explanation. | Selected addition in existing metadata space. Reuse current adapter capabilities; unavailable local session is not proof of remote execution. |
| Appearance — MP12 / M13 | One Appearance entry containing text, tray icon and window behavior instead of three separate navigation destinations. | Selected organization change; retain immediately visible growth choices and actual-size icon previews. |
| System overview — MP12 / R6 | Turn host/process/resource/freshness data into an informative compact overview instead of stacked technical paragraphs; distinguish scopes. | Resource widget exists under Work mode, but the task overview uses a different text-heavy representation. Reuse the data/components rather than add another read model. |
| Views — MP11 / O3 | Delete user-defined views. One-click Reset to the selected preset's saved version when dirty. Keep current filtering and explicit Save semantics. | Delete absent; Reset only inside editor. Add compact Reset beside the current view when modified, not another permanent task-row action column. |
| Footer — M13 / shared shell | Version and corner resize grip only. Remove the repeated host/running line; host health belongs in the overview. | Selected correction; preserve the integrated resize hit target and keyboard behavior. |
| Always on top — M13 / shell | Frequent header toggle in both relevant windows, with overlapping squares and the foreground square filled. State must be clear. | Selected correction; menu currently hides it under Window. Keep floating/pinned lifetime separate from always-on-top meaning. Workspace currently calls always-on-top “Pin”. |
| Inbox / Conversations — MP12 / C1–C2 | Reassess the need for two header entries and remove duplicated navigation/content. | Evaluation requested, not yet approval to remove either direct entry. Candidate: one workspace with explicit Inbox, Conversations and Work sessions destinations and one shared New-request flow. |
| Questions — Q1 / answer-card / taskq §2 | Questions must be authored in a form the UI can present and answer clearly. | Review producer, parser, form, dispatch and receipt together. Fix existing protocol mismatches before adding new rules. Do not silently flatten unsupported question types. |
| Completed tasks — task views / history | Easy browsing of completed tasks and their results/history, with useful search and navigation. | New explicit capability requested. Add a task-history destination without restoring a mandatory Closed counter to the operational strip. |

## Route audit: observed, useful form and proposed organization

All ten top-level Settings entries and four additional work/recovery entries were opened. Pages were read; native/board effects were not executed. Full AX observations are saved next to this report.

| Current route | What the live application actually provides | Target usability / gap |
|---|---|---|
| Appearance | System theme statement; 14/16/18/20px choices, current18px. | Appearance / Text section; preserve preference and readable type. |
| Tray icon | Icon and Summary choices. | Appearance / Tray icon, reuse accepted six-bin renderer and truthful previews; do not revive removed Custom without an owner decision. |
| Window | Auto/Down/Up/Fixed, Pin, Keep above other windows. | Appearance / Window with direct visual choices. Frequent always-on-top remains in the stable header too. |
| Notifications | OS authorization plus Question/Closed/Failed/Attempt-gone × Off/Silent/Banner/Sound and test actions. | Compact event-by-delivery matrix with readable selections; OS permission separately. Tests were not sent. |
| Activity history | Real six-bin event graph and interval controls; long timestamp ranges, technical “bins/failed cap” legend and selected-bin counts. | Concise chart/range/legend with useful detail on demand. This is event statistics, not a completed-task browser. |
| Projects | Boards, task counters, admission and repeated “You asked” paragraphs; Add routes to workspace. | One compact project row with state, direct board and relevant actions; old request prose belongs in details. Preserve explicit roots/setup effects. |
| Agents and Access | Runtime versions/account identity plus “Access and runtime settings…” launcher. Launcher opens a placeholder that directs back to Settings → Agents. | Real capability/effective-policy summary and reachable editor where supported; explain exact unsupported scope once. Remove the circular route, not the required capability. |
| Delivery | Project auto-merge/revision text and Board links; no local editor. | Clear per-project policy/read-only state and supported edit route. Do not promise a visual editor that is absent. |
| Services and support | Same host service/PID repeated seven times under per-project headings; project names clip to one letter in the native narrow screen. | One host service card and common host operations; project registration/log/incident rows beneath it. Full readable identity. |
| About and updates | Tray version plus the same version repeated for each project; Review updates routes to operation review. | One installed/running/available version summary with freshness and exceptional project compatibility only where meaningful. No update was requested. |
| Work mode… | Host capacity widget, CPU/memory/load state and a real limits form. | Label host Resources/Capacity accurately; keep scope and applied/saved state explicit. Absolute local paths do not belong in the normal summary. Form opened and cancelled without edits. |
| Review Work mode scope… | Per-project Pause controls and Pause all; no future-rule/snapshot matching preview. | Admission review with compact project state. Reconcile the claimed full MP2 target separately; changing list filters must not change admission. |
| Emergency stop… | Drain/Full confirmation plus long host/project, acknowledgement and recovery paragraphs. | Explicit consequence review with named host, short mode comparison and relevant running/unknown work. No confirmation was clicked. |
| Recovery… | Per-project Read current recovery controls and long generic cautionary text. | One coherent recovery overview with targeted task/ownership evidence and preserved fences; do not falsely depict unknown as safe. No recovery action was submitted. |
| Inbox | One launched native-request notification; two rejected answer/control receipts; six accepted receipts grouped as Answered. | Prioritized actionable notifications/questions; short outcome and original question/selected answer context. Technical correlation IDs and raw state text belong in details. |
| Conversations | Known native requests, files/drafts and Open session, with a separate next-request composer. | Clear request/session identity and one shared New flow. Full dialogue still requires explicit reconsideration of answer 2; native unread replies cannot be inferred here. |
| Work sessions | Separate route exists; current source lists recorded worker/reviewer attempts from supplied project snapshots. | Keep distinct from owner conversations, with runtime/host/verified link and truthful observed state; history of attempts is not completed-task history. Native route not re-exercised in this round. |
| Completed tasks | No task-history destination in the fixed open-state selector; dashboard explicitly filters closed rows out. Activity history only exposes counts. | Dedicated completed/history view using board-owned records, described below. |

## Additional reproduced findings

- P1 — [Access dead end](05-access-dead-end.png), [workspace.js:59](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/workspace.js#L59) — Settings → Agents → Access and runtime settings opens a placeholder directing back to the same tray route. Supply a real supported destination or a precise unavailable state at the initiating control; A6 cannot be treated as fully proved by route presence.
- P1 — [Inbox receipts](07-inbox.png), [workspace-answers.js:42](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/workspace-answers.js#L42) — Existing receipts for TaskQ#1506/#1562 report “Correlation id is already pending for a different request”. These are observed failures, not a fresh reproduction or proof of a specific current root cause. Investigate exact operation IDs/payloads and outcome recovery before retrying; never silence the guard or resend blindly.
- P2 — [Settings](03-settings.png), [Inbox](07-inbox.png), [app-shell.css:7](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/app-shell.css#L7) — Actions fall below headings and even “Inbox” wraps mid-word at wide workspace geometry. Keep actions in a stable first row, allocate a full-width title row as needed, and preserve accessible targets without arbitrary word splitting.
- P2 — [Return route](home-return-path.json), [app-shell.js:20](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/app-shell.js#L20) — Returning after this inspection required17 Back activations because Back replays visited settings pages. Add direct Home while retaining ordinary Back and per-view context.
- P2 — [Saved views](09-saved-views.png), [editor observation](view-editor.ax.txt), [task-dashboard.js:36](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/task-dashboard.js#L36) — No Delete in saved-view management; Reset is reachable only through the conditions editor. Add user-view deletion in management and one direct dirty-view Reset; do not apply it automatically or clear unrelated views.
- P2 — [Services](06-services-duplication.png), [tray-app.js:445](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/tray-app.js#L445) — Seven project sections repeat the same running host service/PID, while narrow summary names become “a”/“c”. Render one host owner and compact readable per-project information; preserve per-project logs and incidents.
- P2 — [Known-count observation](host-status-known-count.ax.txt), [tray-app.js:423](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/tray-app.js#L423) — “mac · 2 running” combines a local-host label with a total consisting of mac1 and win1. Label global versus host quantities explicitly. [Later capture](08-host-status.png) shows a different truthful unknown state after the live build changed; it is not evidence that the earlier2 was on mac.
- P2 — [Activity](04-activity-history.png), [tray-app.js:270](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/tray-app.js#L270) — Detailed timestamps, technical bin terminology and duplicate legend/selection prose dominate a six-bin graph. Present a concise useful chart/range, keep exact interval and incomplete-coverage explanation on demand, and preserve the shared quantitative geometry.
- P2 — [Inbox](07-inbox.png), [workspace.js:27](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/workspace.js#L27) — Inbox exposes raw host/request IDs and “accepted: doing” without the original answer. Prefer meaningful task/question/result context and move diagnostic identifiers into detail; do not equate a receipt with task completion.
- P2 — [Access screen](05-access-dead-end.png), [workspace.js:141](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/workspace.js#L141) — An unrelated “Native request: launched” notice appears on Agents and operational pages. Scope request delivery status to its request or explicitly linked global activity; do not imply that opening settings launched an operation.
- P2 — [project-blocks.js:61](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/project-blocks.js#L61) — Compact dashboard rows drop the runtime/remote/session marks already assembled for other consumers. Restore the owner's direct verified session affordance beside timing using the existing brand assets and adapter result; classify remote by actual host identity, not missing local-session capability.
- P2 — [tray-app.js:515](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/tray-app.js#L515), [ui-icons.js:7](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/ui-icons.js#L7) — Inbox count exists in accessible label only; the icon-only button renders no visible badge, and Conversations has no unread badge model. Add truthful visible indicators with read/seen semantics and explicit unavailable native-reply state.
- P2 — [Owner report and handlers](https://github.com/taskqorg/taskq/blob/99b2ae1cb3d566eef99ee328f6ac4581fa687cb7/src/task-dashboard.js#L69) — Automatic task previews occur on focus/pointer-over; refresh restores focus and tasks can reorder beneath the pointer. This supports the reported mechanism but is not a controlled causal reproduction. Implement the selected click-only interaction and verify idle refresh, pointer movement, keyboard focus, scrolling and dismissal never open details implicitly.

## Target navigation and organization

A draft information architecture for the correction package, not a replacement approved mock. Preserve every current function or show its explicit unsupported boundary.

- **Tasks:** stable toolbar; one compact system overview; active view/presets and direct dirty Reset; dominant task list. Completed tasks is a first-class reachable view. Details reuse the same readable question/result/history components. Home returns here with the previous filter/selection/scroll.
- **Workspace:** explicit Inbox, Conversations and Work sessions destinations, one shared New-request route/composer and consistent window controls. Inbox is what needs attention; Conversations is known owner sessions/requests/drafts; Work sessions is recorded worker/reviewer execution. The task/event/draft/history distinction remains visible. Whether the menu has one workspace entry or keeps both direct shortcuts is still an owner choice; do not remove MP12 silently.
- **Settings:** Appearance contains Text, Tray icon and Window. Work/Resources separates host capacity from project admission. Projects owns registration/setup; Agents/Access owns real capabilities/policy; Delivery is explicit saved/read-only policy; Service/Support owns host service, logs, incidents and recovery; Notifications owns delivery channels; Activity owns event statistics; About owns version/update provenance. Avoid an intermediate page merely to redirect to another unavailable one.
- **Chrome:** toolbar first, title below when needed, one direct Home, visible truthful unread badges, frequent always-on-top toggle, Close and corner grip. Footer shows version only. Exact crowded/narrow arrangement requires a representative visual pass before spreading it.

## Completed tasks: required scenario and data boundary

The owner explicitly requests this now. Keep it distinct from current Active/All-open filters, Activity event counts, completed native conversations and ended worker sessions.

- Entry: a direct Completed view from Tasks, reusing project/search controls and clear date filtering. Default remains the operational task view; do not clutter its status strip with a new mandatory Closed counter.
- Browse: show issue/provider identity, full title, completion time and truthful disposition. Sort recent completion first; preserve selected projects/search/date and browsing position when opening a task or returning from settings.
- Detail: concise result, PR/commit and available verification links; stage/attempt history and exact board link. A result or successful closure is displayed only with authoritative supporting data. Opening never resumes a task or launches an agent.
- Distinguish completed from cancelled/not-planned, removed-from-queue and unknown closure reason. A closed GitHub issue is not automatically a successful TaskQ delivery; provider limitations remain explicit. Do not lose unknown history just to claim a complete list.
- Source: on-demand bounded/paginated board history using canonical forge/server/repository/issue identity. The current dashboard excludes closed rows (`task-dashboard.js:13`); changing a UI filter alone cannot restore absent historical data. Do not create a second task database from telemetry or infer the list from six histogram bins.
- States: loading, no matches, unavailable/unauthorized source, partial retained coverage, stale data, large history and paging. Show loaded/known counts honestly; no endless scan of unrelated projects or silent retention truncation.
- Acceptance route: open Completed → filter/search/project/date → inspect result/history → return with context → settings and back; cancelled/reopened/unknown examples must remain distinguishable. Validate from real bounded history after the read path is available; no task mutation is needed.

## Questions: producer and UI must share one contract

This is a proposed specification correction package, not a new agent rule applied during review.

1. Inventory actual questions that fail: malformed/unparsed cards, multiple independent questions in prose, single choice, yes/no, free text, and runtime-only permission/attachment prompts. Do not claim all are covered by numbered buttons.
2. Reuse stable task/card/attempt/revision identities and exact source choice numbers/order/meaning. Short question/problem, concise action labels and recommendation; evidence/details on demand. One ordinary choice submits once; a supported custom answer retains its text on failure.
3. Current unparsed-card fallback is read-only (`answer-card.js:86`); consequential close/publication choices deliberately send the owner to the board (`:91–96`). Distinguish those constraints from broken dispatch, stale data and missing layout. Native prompts retain native authority.
4. Where a required input type has no transport/UI representation, choose an explicit product-supported path and amend producer, schema/parser, rendering, dispatch and acceptance together. Do not change wording alone, parse arbitrary prose as execution, or silently discard parts of an answer.
5. Receipts must identify what question/choice was answered and its actual outcome. Investigate the observed correlation-ID refusals with immutable payload/same-ID reconciliation; do not bypass the existing duplicate-operation fence.
6. Qualify the real owner-provided ask card next. Current inspected task snapshot has no open Ask; historical accepted receipts are not a substitute for seeing an actionable question. No synthetic question or answer was posted to production.

## Proposed coherent correction outcomes

1. **Task dashboard and task interaction:** click-only readable details/questions, runtime/session access, manageable saved views/direct Reset and truthful compact system overview. Include shared header/Home/badges/footer/window-control fixes in this outcome and reuse them everywhere.
2. **Operational navigation and settings:** unified Appearance, reachable Access, host-owned Services/Support, readable resources/admission/delivery/notification/activity forms and preserved safe operations. Fix shared layout causes rather than one task per route.
3. **Inbox and native conversation workflow:** coherent destinations, one New flow, readable event/question/result cards, scoped request status and reliable answer delivery/readback; decide the remaining one/two-shortcut and native-dialogue scope questions explicitly.
4. **Completed-task browsing:** authoritative bounded historical read path plus the complete view/result/back-navigation scenario. Separate from the first outcome only because it adds a distinct historical-data capability and acceptance route.

These are review-package groupings only. PM files tasks after the owner agrees. User interaction corrections are captured; exact new visuals, final entry consolidation, and any new dialogue bridge are not silently approved.

## Evidence refresh and boundaries

Retained additional screenshots: [Settings](03-settings.png), [Activity](04-activity-history.png), [Access](05-access-dead-end.png), [Services](06-services-duplication.png), [Inbox](07-inbox.png), [expanded system state](08-host-status.png), [Saved views](09-saved-views.png). Other route observations are AX text, including Appearance, icon, Window, Notifications, Projects, Delivery, About, resource limits, admission, Stop and Recovery. Nine retained screens total; one slot remains for an owner-provided task/question example.

The application updated itself during the audit: first `4.6.34-dev.1792726249`, then `4.6.34-dev.1792726737`, then `4.6.34-dev.1792727275`. Observations retain their own version; they are not one frozen-candidate pass. A transient app restart interrupted a capture; the running installed app was reselected, without requesting a restart, installation or setting change. Current host unknown state is not equated to idle/zero.

Only local review artifacts were written. Navigation and disclosures were exercised; no view Reset/Save/Delete, grant/limit edit, notification test, answer/retry, task/receipt archive/delete, Stop/Recovery/update confirmation or service action was applied. The limits form was opened and cancelled without edits. This remains an ongoing review, not a completed production/native qualification.

## Navigation comparison candidate — 2026-10-23

Review-only prototype: `variants/taskq-review-navigation.html` (SHA-256 `9f1138381389c81f1723ab311b08b46bfae6e7646ad994a49fb27335867588bf`). Both variants use the same illustrative task data and common owner corrections. Left exposes direct Inbox and Conversations entries; right exposes one Workspace entry and then explicit Inbox / Conversations / Work sessions destinations. The owner has not selected either candidate. No canonical decision is changed by this comparison.

Common scenarios: task discovery, direct completed-history entry, unified Appearance route, and an illustrative structured question. Task details open on activation only and overlay the list. The question is a sample, not a live Ask reproduction. Counts, timestamps, capacity and completion dates are fixtures; real issue/PR identifiers supply recognizable context only. Conversation continuation remains native-only under the later owner decision. The badge denotes a modeled request event, not a claimed native unread-message signal.

Browser checks exercised Home preserving a changed query, scoped All before the state facet, one-click Reset, direct Conversations versus Workspace landing in Inbox, draft preservation across section switches, completed rows and a sample answer receipt. A widget-state echo initially collapsed the sample detail; fixed before presentation. JavaScript syntax check passed; final browser console had no warning/error entries. Inspected the main comparison at 1024px, question overlay, workspace, completed list, and Appearance at 360px with 20px text. Narrow toolbar remains one row; checked controls do not overflow horizontally. Five prototype QA captures are separate from the nine native evidence captures.

Limitations: the prototype does not invoke native window sizing/topmost/session opening, file upload, lifecycle operations, transport or board writes. Date/project history pickers and unreviewed settings destinations are placeholders for subsequent design passes; their functionality is not accepted here. No runtime code, service, canonical spec, task or PM state was changed.

## Owner correction: restore visual continuity and review the whole flow

The isolated navigation comparison is withdrawn as a visual reference: it needlessly replaced the frozen palette/row geometry and narrowed the review to one icon. The owner requests all recorded corrections in one connected candidate, quieter task separators, more distinct popovers, visual host/runtime/resource summaries and shared text/file input.

Canonical updates precede the new candidate: taskq.md Product links review refinements; visual-style owns revision continuity/contrast; menu-product MP11–MP15 own behavior; ai-workspace owns shared input and later native-dialogue authority. AGENTS links the same fidelity gate. No new generic approval process or duplicate agent rule set was added. The original frozen control-panels reference is unchanged.

New editable candidate: docs/design/exploration/review-control-panels.html, SHA-256 4568ab09c8fd8b039b5a0fabd900ae0531189c5b3af39fe48d66deb965197a35. Detailed revision verdicts: docs/design/exploration/review-control-panels-review.md. Preview: connected-review.html beside this report; four QA captures under connected-qa/. Root contract remains within 300 lines / 60 KiB. This is a review-only simulation; original live findings remain open until actual runtime fixes and exact-binary verification.


## Window and task-pin review correction

Owner requests clear task pinning without row overlap, considers separate Settings windows, and wants common TaskQ-owned chrome for Conversations and the menu. Source `task-dashboard.js:41/56/58` distinguishes detail-latch and preset pins; no task promotion is implemented there. `tray.rs:1173` maps workspace pin to always-on-top; its workspace constructor at 1248 does not explicitly disable native decorations. These are inspected-source findings, not a new installed-binary qualification.

P-W1–P-W3 in menu-product.md preserve these as review proposals. Updated candidate SHA-256 06a56da23f5ce418abe793a569601b18b3f8739a0ab7f4422bb6f8fa74af174c; current revision checks are in docs/design/exploration/review-control-panels-review.md. Preview window-pin-review.html and window-pin-qa/ sit beside this report. Pin partition, scoped filtering, unpin order, separate Settings routing, draft retention and matching header controls were exercised. Actual native chrome/drag/z-order and canonical persistent task references remain implementation work.


## Owner decision: global always-visible task pins

Separate Settings windows are accepted. P-W1 now records one personal watchlist shared across views and filters, always above ordinary results, with normal compact rows and click-only details. Pin closes the detail card. Pins are independent of preset Save/Reset/deletion and do not duplicate matching rows or broaden ordinary counts. The previous per-view/filter-scoped proposal is withdrawn.

Candidate SHA-256 d58af202f0560d651f7ae83a05f690d2c38714df2f6332be6c756ac86ec1f621; inline global-pin-review.html. Current checks are in docs/design/exploration/review-control-panels-review.md; three captures are under global-pin-qa/. Global view/project/query continuity, no-match filtering, collapsed pinning, unique rows, excluded unpin, reload/migration and independent result scrolling were exercised. Completed/unavailable reference adapters and native behavior remain unqualified.


### Owner correction: compact hosts and shared menu scrolling

Canonical MP14/P-W1 and R-UI3 now require host names with Claude/Codex counts in one default line; resources/freshness disclose on click. The menu has one content scroll for hosts, pins, filters and results. Global pins retain first position across filters but scroll with content; no nested watch/result viewports. Connected candidate compact-host-review.html was inspected at desktop and 360px/20px. Native runtime is unchanged and unqualified.


### Native settings pass and latest owner amendments

A fresh read-only installed-app pass is recorded in taskq-design-snapshot/docs/design/exploration/live-settings-review.md: real data, display gaps, route coverage, ten selected captures and three coherent correction outcomes. Confirmed Access navigation dead end, repeated host Services/version data, clipped service labels, repeated project request text, obscured admission result, long Back trail and an unconfirmed Recovery read. No runtime controls were applied. Owner confirms current views with one event-aware workspace menu entry and no menu Enlarge; prototype single-workspace-entry.html carries those changes. Native unread deduplication and untested operations remain open.
