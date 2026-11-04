# Error and unavailable-surface review

Scope: [the owner's request and A1–A6](https://github.com/taskqorg/taskq/issues/2351). The canonical behavior is [M14](menu.md#errors-and-service-notices); this inventory records where existing information goes. A failed read is a diagnostic; a disabled action's explanation, consent, question, pending receipt or source-specific recovery instruction remains attached to that action. Nothing here changes task state, execution authority or delivery receipts (R6/R12).

| Source and surface | Verdict | Destination or retained protection |
|---|---|---|
| `tray-app.js` `problem`: per-project service error/missing status buttons on Home, Attention, project and Services | Move | One header list, grouped by cause; service Log remains under Services. |
| `home`: expanded dashboard fault stack and its second Attention count | Remove | Header owns error count; setup questions/consent remain in their original section. |
| `projects`: repeated project error/stale-status prose | Move | Header list; project name, Board, Folder, intake mode and dated readback remain. |
| `draw`: Settings unknown, navigation destination and workspace-opening errors | Move | Header list; safe surviving navigation remains reachable. |
| `agentq`: duplicated action-error alert | Move | Header list. AgentQ status, optional setup/consent, setup session and Check again remain with the feature. |
| `attention`/`managerView`: task anomalies, owner questions, failed tasks, board manager report | Keep | These are task information and decisions, not service notices. |
| `incident(top)`: unsolicited local-incident notices on task pages | Move | Header list. `incident` in Services retains Open report, Report manually and the exact local/sent state. |
| `incidentNotice`: public incident-disclosure/opt-in consent | Keep | Services project details; never an error-list decision. |
| `operationBody`: unavailable project, stop/recovery/capacity/operation | Keep | Explains why that reviewed operation cannot run; no automatic fallback. |
| `actionNotices`, `lifecycleWaits`, `lifecycleControl`: action refusal, lifecycle protocol and exact-target waits/readback | Keep | Operation review; preserves outcome-unknown fences and no replay. |
| `stopPanel`, `recoveryPanel`/`recoveryReceipt`: unknown process state, stale review, selection validation, mixed outcomes | Keep | Beside the affected operation and its retained receipts. |
| `admission`, `admissionText`, `balancingSections`, `resourceCapability`/`resourceReceipt`, `resourceMeters` | Keep | Scoped intake/load/permission facts and validation; no capacity or permission inferred from stale data. |
| `runningAgents`: stale host/execution observations | Keep values, move cause | Dated Last known counts during a failed read; no previous trustworthy value means unknown. Definitions and resource-source details remain disclosed. |
| `agents`, `accessFacts`: sign-in/probe/account/access unavailable states | Keep | Capability/settings facts and owner sign-in actions; distinct from process counts. |
| `custom`: invalid range | Keep | Inline validation next to the fields. |
| `history`: unavailable/incomplete histogram, exact interval and missing source coverage | Keep | Historical coverage and source details; no zero substituted for unknown. |
| `addProject`, `setup`: picker/setup failure, original setup conversation and consent | Keep | Specific operation and setup continuation; no external effect hidden. |
| `about`, update controls: version unknown and updater outcome | Keep | Version provenance and explicit update/readback; not a background task-page notice. |
| `notifyRow`/`notifyGroup`: OS permission unknown/blocked/request failure and notification modes | Keep | The settings/permission operation, with OS-setting destination. |
| `project`: empty task list, configured mode, capacity and service details | Keep | Task/feature facts; its duplicate unsolicited error and incident stack moves. |
| `workspace.js` `pages.conversations`: per-project activity/result-collection failures | Move | Header list; one collection-coverage qualifier retains unknown totals. |
| `pages.conversations` History sources: repeated report-collection diagnostic per project | Move | Header list; source name and loaded count stay disclosed. |
| `pages.conversations` footer: local persistence/navigation/AI errors | Move | Header list. One Data from time and thread-operation feedback remain. |
| `draw`: the same global alerts on non-Conversations routes | Move | Same header list on every workspace route. |
| `route`: failed renderer/recovery view | Keep | Safe Home/Back/Close/Refresh recovery, because no content could be rendered. |
| `task`, `linkedTask`, `taskWait`: unavailable/stale task card, source, question/answer and parked state | Keep | Exact selected task and Retry card read; answer/assign freshness checks remain. |
| `message`, `sessions`, `ids`: unavailable thread/session, native IDs and read-only work-session status | Keep | The selected source and its recovery destination; no guessed session. |
| `threadNotice`, `answerComposer`: delivery refusal, draft/file validation and pending receipt | Keep | The affected thread/composer, preserving drafts, files and exact operation IDs. |
| `conversations.js` catalog error in `body` feedback | Move | Header list; catalog read time and refresh action remain. |
| `body` draft recovery, save failure, cross-thread pending operation and recovered receipt controls | Keep | Draft/operation recovery with Check original delivery/Retry native save. |
| `bodyRead`, `linkedRead`/`linkedTools`, task/history reads, `transcript`, `closed`/`closedRead` | Keep | Selected-source loading/refusal/overflow and Retry/Latest; retained source is not replaced with an empty successful read. |
| `status`, `activity`, `capability`, `runtime`, `launchProblem`/`launchControls` | Keep | Native execution/capability and launch validation; no permission or runtime fallback. |
| `body` file preview errors, archived/deleted source, source-position/selection recovery | Keep | Specific selected source/draft, with preservation and recovery instructions. |
| `reports.js` `sources`/`reason`: collection failures consumed by workspace | Move | Header list through snapshot report reasons; report rows stay retained by existing cache. |
| `section`: selected exact-SHA result unavailable/read failure and Retry source read | Keep | Cannot review a source that did not load; old content and its SHA remain distinct. |
| `capability`, `cardRow`, `receive`/`feedback`/`readFailure`: result decision validation and rejected/partial/unknown receipt | Keep | Beside decision controls; created held tasks and uncertain outcomes cannot disappear into a background list. |
| `app-shell.js` `bar`: unavailable route title | Keep | Navigation fallback title. The shell renders one supplied errors entrance; it creates no second diagnostic store. |
| `task-dashboard.js` `list`: failed-project paragraphs | Move | Header list. Project group counters retain their unknown qualification and task rows remain visible. |
| `notification-panel.js` entrance: second error glyph | Remove | New/Needs me stay independent, with exact loaded/unknown accessible counts; the header errors entrance owns the diagnostic glyph. |
| `notification-panel.js` tools: repeated per-project collection reasons | Move | Header list. Explicit Mark read/Clear read refusals remain next to those operations. |

Verification uses the same invented nine-project fixture in browser regression and native snapshot routes `errors` and `errors-conversations`. The regression covers recovery without acknowledgement, retained rows and host age, unknown without evidence, stable page geometry and questions staying in Needs me. Native evidence and exact outcomes belong in the task's revision-specific evidence directory, not this specification inventory.
