# Tray and workspace placement

Approved design target from the [#1443 owner plan](evidence/1443/owner-plan.md)
([approval](https://github.com/taskqorg/taskq/issues/1443#issuecomment-6311797833))
and the #1462 owner answer (option 1 plus any-runtime sessions). This is not
shipped behavior: runtime stays as [taskq.md § 4](../../taskq.md#4-service) and the
linked contract details describe until the implementing tasks pass (R8/R12/R13).
Where an older clause in [menu](menu.md), [adaptive menu](adaptive-menu.md),
[answer card](answer-card.md), [AI requests](ai-workspace.md) or
[app UI v2](ui-rules-v2.md) places dialogue in the menu, this page wins.

Implementing tasks: N [#1463](https://github.com/taskqorg/taskq/issues/1463)
navigation, W [#1464](https://github.com/taskqorg/taskq/issues/1464) answers and
requests, O [#1465](https://github.com/taskqorg/taskq/issues/1465) reviews,
P [#1466](https://github.com/taskqorg/taskq/issues/1466) panels,
M [#1467](https://github.com/taskqorg/taskq/issues/1467) messages and archive.
They build their own screens directly on the [foundation](app-foundation.md):
M3/M4 are folded into #1463–#1467, #1463 supplies the shared shell, and M2
calibration runs in parallel. Toggle selection reuses #1455.

## Contract detail

- TW1 Navigation (point 1; V01–V06): every screen has one fixed bar: leading Back, one route title, trailing actions, then the separate window-control group. No separate Back row or content title. Roots: tray Home and workspace Messages; a root shows no Back, and no Back crosses windows. Nested project and settings routes show only linked ancestors: project overview: `Projects`, project Tasks/Work mode: `Projects › <project>`, settings category: `Settings`. Breadcrumbs end at the parent; the current page appears once as the title. A flat list has none. Back restores the invoker's focus, scroll, filters and drafts (V06/V07). Focus rings use `:focus-visible` for keyboard controls; programmatic heading focus shows no box.
- TW2 Placement (points 2 and 4): the menu holds panels, then tasks, then setting links. It holds no composer, conversation, answer card, review or inline confirmation. Task answers, AI requests, the messages list and operation reviews (service, resources, emergency, recovery, update, setup) live in the workspace. A menu click opens the exact workspace context and never sends, answers or confirms. The route families and destinations are the [owner plan review](evidence/1443/owner-plan.md#whole-surface-usability-review); Work mode stays the M12 editor.
- TW3 Panels (point 3): Home starts with compact panels: executing workers and reviewers per host and runtime ([running agents](menu.md#running-agents)), work controls, active faults and Messages. Counts are actual executions, deduplicated by board/task/execution. Slots, reservations and Doing/Review task totals stay separately labelled. Stale (over 60 s), missing or partial data shows unknown with its reason, never zero or idle. Empty panels take no rows; the task list follows.
- TW4 AI requests (#1427 kept): the workspace request composer names the project and the runtime. Any available agent runtime opens the session, Claude or Codex; a host with only one runtime uses it and shows its name. With both available the owner picks one; no silent fallback. With none, Send is disabled and states why. Send writes one `<install>/setup/<id>.json` with that `runtime` and launches one visible session through the setup path; the session owns the dialogue. TaskQ stores no conversation.
- TW5 Messages (point 5): the badge counts pending entries, not processes, slots or tasks. Entries are open `ask` questions (identity: board, task, question revision) and owner-session records with an unread outcome: `launched`, `failed` (no start) or `interrupted` (identity: host, request id, state). Opening an entry marks it read; an unanswered question stays pending until the board leaves `ask`. Drafts and routine updates add none. Archived entries leave the list and the count; a new question revision is a new entry. Unread and archive marks are local view state in `<install>/messages.json`, never task truth (R1). A stale or incomplete source shows the badge as unknown with its reason.
- TW6 Archive and delete (C7, point 5): any entry can be archived, active and unresolved ones included. Archived opens the archived list; Delete removes one archived entry, Delete all removes every one, permanently, with no undo or trash. Delete acts only on TaskQ message records: the request file, its `<install>/setup/<id>/` folder (brief, launch log, owned attachment copies) and its marks. Archiving a question hides only the local entry; the board task remains.
- TW7 Execution boundary (C7): archive and delete never stop a session, end an attempt, change a task or touch source files or native Claude/Codex chats. A request not yet launched (`state` `pending` or `launching`) refuses Delete with that reason, since the service still needs it. Delete validates that each path lies inside `<install>/setup/`, then reads back its absence; a failed or partial delete names what remains. No hidden recoverable copy is kept.
- TW8 One control, one rule (point 7): a toggle exposes `aria-pressed` with the one selected outline (#1455); a current destination keeps `aria-current`; links and action buttons never show selected state. The task-list state filter is the counter strip (M3): pressed capsules, OR within states, no checkbox group; this replaces the v2 checkbox and select-all clauses (V30/V31). Answers keep source option IDs and revision/operation-ID guards ([answer card](answer-card.md#contract-detail)); only their placement changes. DashQ keeps its own placement.

Acceptance: each implementing task repeats its affected flow against TW1–TW8 and
the [interaction table](evidence/1443/owner-plan.md#interaction-differences-to-remove);
the last one reviews the combined tray and workspace against all seven points.

#1467's [owner answer](https://github.com/taskqorg/taskq/issues/1467#issuecomment-6320097999)
confirms TW5–TW7 message records only, the prelaunch deletion refusal and #1464's
attachment deferral. Native conversations stay in their runtime. Local metadata
keeps content-free identity digests after deletion to prevent an unresolved
question reappearing or a deleted request ID launching again. An incomplete
deletion keeps only its request identity and retry status, never a history copy;
Delete all reports each refused or incomplete item separately. Command history
and composer drafts remain separate from these message records.
