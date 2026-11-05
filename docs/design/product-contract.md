# TaskQ product contract

Product requirements linked from [taskq.md](../../taskq.md#product).

## Contract detail

Looks/sound: one line each (R13).
- Home: current work, unresolved questions. Communication: [Notifications](menu.md#m14-notifications). Read/clear changes no task state.
- Errors and service notices use one steady header icon and deduplicated list in tray and Conversations; failed reads retain aged last-known data ([attention](menu.md#errors-and-service-notices)). R6/R12 freshness and action gates still bind.
- One [Conversations](reports.md) entrance; New and Needs me use icon/count counters in the top task bar. One chronological reader opens exact target (including receipt tails), saved passage, else latest mixed history; bounded paging keeps source positions. Cache budgets evict projections, never restrict readable history. Ordinary completions need no Accept.
- [Promo A](../video-experiments/planning-802/approval.md).
- Approved [targets](README.md) and [navigation](tray-workspace.md): parent-only breadcrumbs, keyboard-only rings, view-only counter-strip filter. [Foundation](app-foundation.md): M3/M4 first, M2 parallel, M5 after; CoreQ only. Runtime changes need implementation and proof (R8/R12/R13).
- UI: Preact/Signals/Wouter O1–O3; [UI30 targets](ui-polish.md): palette A, exact counts ordered Ready+Blocked / Ask / Work, event stack A; no Custom. Native proof pending (R12).
- Menu, Settings, View and Conversations use ordinary opaque native OS titlebars and integrated frames, drag areas and Close, without duplicate app Close. Conversations safe Reload preserves drafts/files, exact sources, anchors and receipts without replay; [unified reader](tray-workspace.md#unified-task-and-session-reader) owns rows, history, provenance and exact event jumps.
- Conversations preserves scope, selection and paragraph offsets within 2 CSS px with one anchor owner per viewport; saved logical-pixel sidebar width/visibility and display-clamped native geometry survive reopening ([reader and layout](tray-workspace.md#unified-task-and-session-reader)).
- Compact task rows share numbers-only timing: elapsed since first accepted worker ownership, subsequent actual state-change count, current-state elapsed; unknown boundaries stay explicit ([task rows](project-blocks.md)).
- Closed pins: unpin after first open or 24 h; [P-W1](menu-product.md#windows-and-task-pins).
- Settings keeps twelve destinations with bounded summaries, useful Balancing/Recovery state and direct existing reviews; client facts once, project details available ([M8/M12](menu.md#m8-implementation-boundary)).
- Every UI task identifier displays `project#NNN`; lists and headers share a keyboard/touch-accessible copy widget for it; status-bar versions reuse it with their exact displayed text ([workspace](task-workspace.md)).
- Home agent totals: confirmed lower bounds, accessible Partial/Unknown explanations without a standalone compact label, shared runtime icons, host age and cause ([M3](menu.md#running-agents)); Sigma total first, subtle Held outline and scoped frozen-review Start ([K-A3/K-A6](task-counters.md)).
- Shared relative times, exact on hover/focus, future and unknown explicit: [D-A19](ai-workspace.md#conversation-time-d-a19).
- Menu/View/Settings wrap, no horizontal scroll; compact rows, growing input; M2 rail; M3/M1.2 opaque sticky Home host strip starts at its pinned position. Home has no Tasks heading; [filter row](menu.md#m3-navigation-and-tasks) reuses current/previous views and grouping.
- Standalone: [onboarding](standalone-onboarding.md); [telemetry](standalone-telemetry.md): local SQLite/WAL, 30 days, async only under revocable `taskq-telemetry/1`. `profile export`: analysis; `guide`: agents. Migration keeps legacy rows (R1).
- README: header, three-part setup (install phrase; repository and Issues-as-board permission, existing or new private; optional SDD request), purpose, three visual examples, shots, disclosures and final line: [README rules](readme-plan.md#contract-detail).
