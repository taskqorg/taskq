# Menu design session: exploration

This is working context, not a blanket product approval or implementation plan. Explicit selections are linked below; other proposals remain exploratory. The final owner request authorizes one PM handoff task and PM creation of implementation tasks; it does not start implementation or application rollout.

## Current outcome and phase

Design exploration and the closing Conversations walkthrough have converged. The owner says the result looks good and requests a complete final package delivered in one TaskQ task to Project Manager, who will form implementation tasks. The existing implementation brief now owns the handoff; preserve bounded unresolved product choices and native/backend evidence gaps.

## Owner feedback to preserve

- Use simultaneous visual comparison; no bottom carousel that forces repeated scrolling.
- Explore compact, editable saved views and pinned presets. Keep the active selection clear and configuration out of the normal working view; preserve state during navigation.
- No task-menu cue or separate right-hand ellipsis. Latest exploration uses the compact forge/issue reference as a hover/focus preview anchor, with click/keyboard access to details and actions. Keep extra information out of the repeated row.
- Review the whole repeated row, not just an isolated control. Remove repeated words and use shared context or clear symbols without hiding identity, units, uncertainty or actions.
- Explore compact task-stage history with elapsed current-stage and total time; source/granularity and stall semantics remain unresolved.
- Keep conversations, conversation-list navigation, drafts and archives in the workspace exploration. Preserve the distinction between owner conversations and worker/reviewer attempts.
- Review every proposed candidate against the specs before showing it. The owner chooses among valid design trade-offs; owner feedback is not a replacement for agent review.

These are inputs for exploration. Saved-view defaults, exact navigation/widgets, row composition, settings taxonomy and timing semantics are not approved product decisions.

## Selected in this session

The owner explicitly selected the left filter-editor variant: a separate overlay panel, keeping the task list in place. Canonical target: taskq.md Product and [MP11](menu-product.md#decisions). Approval covers placement only, not every field, label, save/restore mechanism or demonstrated interaction. Continue within this choice; the inline alternative is historical unless the owner reopens it.

The owner selected direct Inbox/Conversations and an extensible Settings menu ([MP12](menu-product.md#decisions)); only essential actions belong in the header. The owner also requested an always-accessible Close control and dragging from unpinned mode that enables Pin on the first actual drag, with the last chosen mode retained ([M13](menu.md#m13-pinned-window)). The consolidated-menu alternative is historical. Taxonomy and counter semantics remain open.

The owner requested multi-selection of task states/projects and visible choices or previews instead of dropdowns for short option sets ([MP13](menu-product.md#decisions)). The default sort should be understandable from the filter order; exact sorting policy is being explored. Preserve both combined and project-grouped task views. Pinned-window growth choices (down/up/fixed) and visual tray-icon settings are next settings explorations, not selected defaults.

The owner accepted the corrected filter: All with a scoped total, fixed state order, single-column state choices and no reorder mechanism. MP13 records this scope. Acceptance does not add approval for every sort, save or persistence policy shown in earlier prototypes.

## Rule and artifact pointers

- Shared [workflow gate](https://github.com/taskqorg/specq/blob/main/design/workflow.md#before-presenting-a-candidate), [D1–D14](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md) and [F1–F6](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md).
- Product [menu](menu.md), [visual system](visual-style.md), [operator checks](operator-ui.md), [AI workspace](ai-workspace.md) and [UI acceptance](ui-acceptance.md).
- Current editable [exploration prototype](exploration/menu-session.html) and [revision review](exploration/menu-session-review.md). Prototype actions/data are illustrative; no live board mutation or native qualification is claimed.

## Continue from here

The owner said the direction is close and asked to continue logically. This supports further exploration, not approval of all copy, controls, timing or persistence semantics.

Session route:

1. Task list and on-demand preview: current direction; total-age visibility remains open.
2. Saved views and filtering: corrected selector accepted; exact sort/save policies remain open.
3. Window settings: immediate animated previews and integrated corner grip accepted in direction; native geometry remains unqualified.
4. Current round: connected task/settings and conversation control panels, including icon previews, direct local choices and retained conversation tools.
5. Review complete journeys, resolve open choices, then obtain explicit approval before canonical requirements, implementation plan or queue tasks.

The previous filter-editor round used the same six-task fixture and shared view state in both panels. Only editor placement differed; filtering functions were deliberately identical. The owner could not see a difference because the comparison collapsed both editors after shared updates. That presentation failed, regardless of the earlier functional checks.

The corrected comparison has top-level review controls for working mode and opening both editors. Editing mode holds the popup open while the inline form is used; restored state keeps this review scene visible. Apply/Cancel return both panels to working mode. These synchronized controls and held-open popup are demonstration behavior, not application requirements.

New [view-editor candidate](exploration/view-editor.html) and [review](exploration/view-editor-review.md): project, state, sorting and grouping; apply/cancel; save existing or save-and-pin a copy; restore saved criteria; retain temporary criteria and quick search separately for each view during switches and settings navigation. These are proposed behaviors, not accepted product requirements. Quick search is working context, separate from saved preset criteria. The earlier [grouped/flat comparison](exploration/menu-session.html) remains available.

Current candidate keeps forge/issue identity, title, state symbol and current-stage age in the list. Runtime, host, total age, ordered stage durations and a local detail action appear in the preview. Whether total age and stage count must also remain visible in the list is open; moving them is not an approved removal.

Research inputs: [Linear custom views](https://linear.app/docs/custom-views), [display options](https://linear.app/docs/display-options) and [GitHub saved views](https://docs.github.com/en/issues/planning-and-tracking-with-projects/customizing-views-in-your-project/managing-your-views). Reuse the distinction between filtering and display, favourites and explicit save state; do not import their full navigation or data architecture.

## Previous round: header navigation

[Candidate](exploration/header-navigation.html) and [pre-presentation review](exploration/header-navigation-review.md). Both alternatives retain the selected overlay editor and the same task rows. Top review controls show home or navigation in both panels together.

- Direct access: Inbox, Conversations and Settings in the header. Inbox and Conversations require one activation from the task list.
- Consolidated menu: one Menu entry replaces those three shortcuts. Inbox and Conversations require a second activation inside Menu.
- Both proposals retain named-host status, Pause, Emergency stop and Pin. Six global task-state counters occupy the second header row and lead to display filters. CPU/memory move into host details; this placement is proposed, not approved.
- Proposed groups: Work (Work mode, Projects); Environment (Agents, Services); Personal (Appearance, Notifications); Support (Activity history, Diagnostics, About). The consolidated alternative adds Communication (Inbox, Conversations). This taxonomy remains open.

The local preview preserves filter/search through the exercised Settings, Appearance, Inbox and task-detail return paths. Conversation and Work mode destinations are separate preview sections, not implemented native windows or complete editors. Service actions and project registration remain visibly unavailable with reasons; no live effects occur.

Direct access is now selected under MP12. Counter semantics and settings taxonomy remain exploratory.

## Current refinement: window behavior

[Selected-window prototype](exploration/window-behavior.html) and [review](exploration/window-behavior-review.md). A single direct-navigation window now includes Close in its shared header. Header dragging proposes a small movement threshold so a click or jitter does not pin; actual dragging switches to pinned mode in the same gesture. The preview bounds movement to its own stage; it does not simulate native displays or Windows Snap.

Local browser checks cover Pin, Close/Escape, outside click in either mode, reopening and mode/filter/search restoration after reload. The pointer-drag tool produced no observed movement, so actual drag-to-pin remains unqualified; model checks do not replace that evidence. Native fixed-header scroll, blur, work-area placement and per-display persistence remain untested. Runtime source is unchanged and still permits dragging only after manual Pin.

## Previous round: task selection and ordering

[Candidate](exploration/task-selection.html) and [review](exploration/task-selection-review.md). Eight illustrative tasks now include Doing and Review in both TaskQ and AgentQ, so project-local and combined ordering can be compared meaningfully.

- State buttons support multiple choices immediately. Project choices also combine; OR within a facet, AND across facets. Empty selection means All; quick search remains separate working context.
- The overlay offers visible project/state choices, three sort options and combined/project-grouped display. State choices now use one column with no reorder controls. Changes preview immediately; Done closes the overlay, Save updates the preset separately. Exact save/preview behavior remains proposed.
- Owner correction: changing state order was never requested. The two-column arrow editor was an agent scope error and a misleading spatial sequence. It is removed, including stored custom order. Do not replace it with hover arrows or drag handles unless the owner explicitly reopens that need.
- State order is fixed under MP13; the candidate retains attention first, then Doing/Review/Ready/Blocked, with priority and issue number as tie-breakers. Time in stage and Total time remain explicit alternatives. Both display modes use the same comparator; the full default-sort policy is not approved.
- All now shows the total for current projects/search before state selection; per-state counts use the same scope. These are stable filter options, including zero matches, not the global K status summary. Legacy prototype filters/search/grouping are retained while custom order is discarded.
- Appearance demonstrates visible font/tray-layout buttons. Actual native tray-icon previews and the full settings redesign are not delivered by this round.

Previous exercised paths preserve states/projects/group/search through Settings/Appearance/Back, preset switches, explicit save and reload. The live editor reacquires its anchor after list rerender; otherwise it jumped above the window. Current correction checks are in the linked review: All totals and scope, one-column selection and no reorder affordances.

## Previous round: pinned-window growth

[Comparison](exploration/window-growth.html) and [review](exploration/window-growth-review.md). Three simultaneous previews show top-fixed growth down, bottom-fixed growth up and fixed geometry. Top controls change the same illustrative list between two, four and eight tasks. Four tasks are shown initially so the behavior differs immediately; the two-task scene establishes a shared baseline.

The compact preview intentionally isolates geometry and scroll ownership; it is not a replacement reference for full menu navigation or task actions. Header pin/close glyphs are static in this comparison. Numeric stage bounds and the two-task fixed height are demonstration dimensions, not proposed product defaults or a native sizing algorithm. Scenario changes reset scroll for comparison; this does not define live refresh behavior.

No growth mode is selected on initial load. Local preview selection is not product approval. The proposed universal Down default is withdrawn: the owner requires Auto to follow actual menu opening, independent of OS. This isolated diagram was the wrong next deliverable; existing behavior needs a clear settings interface. Exact fixed-size selection, manual resizing, per-display persistence, edge fallback and transition while interacting remain open.

Browser evidence covers edge invariants from two to eight tasks, overflow, keyboard End reaching task #113 in the fixed preview, selected-mode restoration after reload, both themes and large/narrow type. Native OS resizing and work-area behavior remain unqualified.

## Current round: Window settings

[Candidate](exploration/window-settings.html) and [revision review](exploration/window-settings-review.md). The owner reports existing growth behavior works; this round designs its settings, not a replacement native sizing algorithm. M13 records the corrected placement rule and requested footer/grip. OS-specific defaults are withdrawn: Auto follows the actual opening side supplied by native placement; explicit Up/Down/Fixed choices override it. Pin/drag retain the active direction and explicit setting.

The actual menu now contains a proposed Window settings page with four directly selectable miniature previews, a persistent status footer and resize grip. The miniature shows the icon and window position; the previews animate immediately and repeat without hover (static with reduced motion). The resize mark is integrated into the lower-right corner with no square chrome; the32px target and keyboard semantics remain. The4s loop and exact grip styling are candidate details. Version is explicitly unknown (`v—`). The Window section's grouping, final labels and miniature styling remain candidates, not approved taxonomy.

Review controls simulate opening up/down outside the product. They do not locate a native tray icon or qualify work-area geometry. Browser checks cover Auto resolution, preserved overrides, Pin/Close/reopen, saved selection/dimensions, narrow/light layout and fixed chrome during content scrolling. Initial keyboard shrinking was exercised; increasing after reload timed out in the UI tool and remains unqualified. Pointer resizing and native behavior remain untested.

Next: review this settings presentation, then actual tray-icon previews/settings grouping and the conversation workspace. Task total-age visibility and native hover/drag/placement remain open. No queue tasks, runtime edits, commit, push or deployment are authorized by these prototype refinements.

## Current round: connected control panels

The owner said the immediate animation/corner grip is much better and requested the same treatment across settings, icons and Conversations. Conversations must function as a compact operator panel, with clear creation, selection and management tools and a comfortable resizable window. C2 records this target; the request does not approve every proposed control or taxonomy.

[Connected prototype](exploration/control-panels.html) and [revision review](exploration/control-panels-review.md). Task menu and workspace are shown together; top review controls stay above them. The menu keeps the selected filter model and overlay. Window previews/corner grip are retained. Interface settings propose Tray icon, Text, Window and Notifications; Work, Environment and Support routes remain available.

Existing conversation, custom-icon renderer/editor and agent-policy components are reused within shared chrome. Conversation list/search/New/Archive, rename, context, attachments, draft, numbered answers, cancel acknowledgement boundary and explicit detach ownership are represented. Work sessions remain distinct. Narrow windows use List/Back; the composer/footer remain outside transcript scrolling. Enlarge/restore and keyboard workspace resizing are exercised. Task filters, conversation drafts and saved icon state share one persistence envelope instead of overwriting each other.

The Work mode screen currently explores host Auto/Fixed and maximum only; it is not the complete MP2 rule/snapshot scope review. Projects, Services, Activity, Diagnostics and About retain compact routes; native picker/service/update actions remain unavailable. Saved custom icon survives reload; unsaved icon editing survives navigation, with reload behavior still open. No production policy or runtime request is sent.

Current review covers desktop dark16 and constrained light20, draft/filter continuity, create/rename/archive/restore/send, icon save and agent draft review. It does not qualify native drag/resize/placement, attachment adapters, detached OS windows or every advanced failure fixture. The browser comparison exempts interaction with the adjacent workspace from simulated popover blur so one review panel cannot move under the pointer; native blur semantics remain M13.

Next: owner walkthrough of these connected surfaces; refine information density, settings grouping and conversation tools before implementation requirements/tasks. Do not reopen accepted filter order or introduce task-row ellipses. Total task age/stage count visibility remains unresolved. No implementation, task creation, commit, push or deployment is authorized by this design round.

## Closing walkthrough: Conversations

The owner accepts the overall direction and asks to finish the create/switch/return-to-draft/archive journey, then converge on the design package. No new design branches. General direction is accepted; incidental controls and policies are not blanket-approved.

Baseline route exposes: New retains an old search that hides its new row, does not focus the composer, and shows redundant Editing/empty-helper text. Archive changes the entire list to Archive, adding a return step. A chat-first save also contains undefined menu summary fields and fails to restore after reload; a menu-first save supplies those fields. Fix the JSON snapshot boundary and recheck this first-action case.

Proposed refinement for walkthrough: New clears the list search and focuses its own draft; returning to an editable draft focuses the composer. Archive stays in the current list, selects a remaining visible conversation and offers Undo; Restore opens the restored conversation. Show draft identity without repeated internal state labels. These details remain review candidates, not new approved requirements. Preserve active/unresolved archive guards, attachment state and all independent conversation context.

Walkthrough completed in the browser: create after a nonmatching search, independent drafts, return with composer focus, archive without switching the whole list, Undo, explicit Archive/Restore, window close/reopen, and reload. The first-action save failed because optional menu summary fields were undefined; the JSON boundary now removes absent fields. Chat-first reload retains the draft without needing an earlier menu change. Focused regression check: `node docs/design/exploration/control-panels-check.cjs`.

Final refinement also preserves each list's selected conversation across Archive/Recent switches and keeps selection when clearing search. Narrow Conversations has direct List/New actions; New no longer requires returning to the list first. Editing/empty helper copy and duplicate row-state words are removed; an unsent draft uses its first line as a temporary visible title, while explicit titles stay unchanged. History deletion remains available under History. Native Escape after reload was not requalified because the UI tool timed out; Close/reopen passed.

## Finalization checklist

- Overall direction is accepted. Do not restart layout exploration or reopen fixed state order, overlay filtering, direct navigation, immediate growth previews or the corner grip.
- Review this closing walkthrough's details once; promote accepted refinements into C2/C7/C9 and the corresponding M/MP/visual references. Prototype interactions alone are not approval.
- Resolve the already-open task-row total age/stage-count presentation and exact sort/save defaults in one focused final pass. Keep settings taxonomy at the current candidate unless owner feedback identifies a concrete problem.
- Freeze one reference revision and connect scenarios, component states, icons, persistence and acceptance IDs. Reconcile the existing preserve/relocate inventory; Work mode's full rule/snapshot editor and unconnected service routes cannot silently disappear behind this overview.
- Update the existing implementation brief/backlog, not a parallel plan. Separate native rendering/window and backend acknowledgement/receipt gates from owner design choices. Do not create/start tasks or change runtime until separately authorized.

## Final package and PM handoff

The latest owner request authorizes packaging all session materials, one TaskQ PM planning handoff and delivery to PM for task decomposition. Freeze the connected reference and closing journey; reconcile the existing brief/backlog, not a parallel plan. No automatic implementation start, deployment or permission changes. Earlier exploration-only authorization statements describe their own rounds. The final brief is the current handoff entrypoint; its open-choice table survives general visual approval.
