# Task selection: exploratory review

## Owner correction: fixed state order and All total

Current SHA256: `714af077649a705c6faefbffba03ebdf12fc943698bfe3335b0c9d503518ce78`.

The previous candidate added unrequested state-order editing. Its two-column layout made vertical arrows move items sideways. The earlier functional ordering checks did not establish acceptable usability; that presentation is rejected. Removed the controls and handler entirely, rather than hiding them on hover or adding drag-and-drop. MP13 now records fixed state order and the All counter.

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow / scope | No reorder UI, handler or customizable order remains. State choices use one column at both widths. Legacy prototype snapshots retain filters/search/grouping while discarding custom order | Pass for source; migration helper checked |
| D8 All total | Initial All 8; Doing + Review yields four rows while All stays 8. TaskQ + Keep yields All 1 and task #107 | Pass in browser and model checks |
| D8 scope | Source-derived checks: TaskQ total4; TaskQ+AgentQ with Keep total2; state selection does not reduce All | Pass |
| D3/D6 layout | Dark 470/16 and light 320/20 screenshots inspected. Narrow app clientWidth=scrollWidth=318; all seven header selectors stay on one row. State selector has one 292 px column and zero arrow controls | Pass for browser fixtures |
| Selection continuity | Doing/Review multi-selection still works; project selection/search update counters and rows. Fixed ordering and legacy order removal pass model checks | Pass for exercised paths |
| Syntax/runtime | Script syntax and inspected route console pass | Pass |
| Native acceptance | OS bounds/DPI, native scrolling/focus, screen reader and real drag/hover | Not tested |

Only the requested correction is included. Full sort/group/save policy and subsequent settings previews remain exploratory. No application runtime or installed settings changed.

## Superseded revision evidence

The following record describes the rejected reorder candidate and its earlier source. Its ordering controls and usability claims are not current targets.

2026-10-22. Source: [task-selection.html](task-selection.html). SHA256: `bcd426a12f9d5eeb7ed96784ca300dab071accd008c7c8546d989422228abb6c`.

MP11 overlay placement, MP12 direct navigation and M13 window controls are retained. MP13 records the owner's multi-selection and visible-choice direction. Exact facet semantics, sorting, grouping, immediate preview and persistence are candidates, not blanket approvals. Fixture expanded to eight tasks to include Doing/Review in two projects.

## Candidate behavior

Multiple states form a union; multiple projects form a union; projects/states/search intersect. Empty facet selection means All. State-order sorting first partitions selected questions/failures, then uses visible state order, priority and issue number. Project grouping preserves the same comparator inside each group. Duration alternatives retain attention first. State reordering cannot move ordinary work above attention.

Counts ignore the state facet and include current projects/search. These are filter options, including zero-match choices, not a replacement global K summary. Selecting a zero-match facet can show an empty result; it does not claim the task system is empty. Unknown/live data behavior is not simulated by this fixture.

## Before-presentation evidence

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow / MP11–MP13 | One selected window; overlay editor retained; no rejected placement/menu alternative. Project, state, sorting and grouping options are visible buttons, with no native selects in the editor | Pass |
| D5/D8 multi-selection | Doing then Review shows #105/#107/#108/#103. TaskQ plus AgentQ keeps those four tasks. Toggling facets updates the list without Apply | Pass for local candidate |
| D2/D6 ordering in both displays | Move Review earlier: combined order #108/#103/#105/#107; grouped TaskQ #103/#107 and AgentQ #108/#105. The same comparator governs both | Pass; model checks plus browser route |
| D8 duration alternative | Time in stage in combined display shows #105/#107/#108/#103. Grouped display retains #107/#103 for TaskQ and #105/#108 for AgentQ | Pass |
| D13 attention | Model check confirms #104/#106 precede ordinary work under duration sort. Order controls do not cross the attention boundary | Pass for fixture; default policy remains proposed |
| D8 truthful counts | Projects TaskQ/AgentQ exclude PilotQ's failed task. Search Keep yields Doing=2, Review=0 while preserving both selected states | Pass for scoped facet counts |
| F3 continuity | Selected states/projects/order/group and Keep search survive Settings → Appearance → 20 px → Settings → tasks. Save, Attention → All tasks and browser reload retain the configuration/query | Pass for exercised local routes |
| D8 focus | Font choice retains focus on the chosen size. Facet/sort updates retain focus in the overlay; disabled moved-direction focus falls back to that state's toggle | Font and sort observed; full traversal not tested |
| D8 overlay stability | After live filter/sort/group updates, the popup remains 3 px below a newly acquired Edit anchor; Edit aria-expanded stays true at both widths | Pass after stale-anchor correction |
| D3/D6/D9 density/type | Dark 470 px/16 px and light 320 px/20 px home/editor screenshots inspected. Narrow app scrollWidth=clientWidth=318; all seven state controls stay on one row. Editor width308, clientWidth=scrollWidth=306; state controls reflow into one column | Pass for fixtures |
| D4 concise copy | Projects, States/order, Sort and Display replace verbose explanatory legends. No repeated state names added to task rows; titles retain full block width | Pass for inspected candidate |
| MP13 settings illustration | Appearance has visible 14/16/18/20 and Icon/Summary/Custom choices; selected 20 then 16 through direct buttons | Pass for local selection; icon pixels not previewed |
| Model/source checks | Executed source-derived checks cover multi-facet matching, both grouping orders, duration sorting, attention boundary and duplicate rejection. Script syntax and inspected browser console pass | Pass |
| Native/full behavior | Actual hover/drag, screen readers, coarse pointers, OS bounds/DPI, native scroll ownership and live board transitions | Not tested |

The first exact-render review found a detached Edit anchor after a live list rerender: the popup jumped upward and its expanded state became false. Corrected by reacquiring the current anchor and restoring expanded state before positioning. Rechecked both widths after the correction; the earlier position was not accepted as a valid alternative.

Long narrow overlays use normal document scrolling in this content-sized preview. That does not qualify native viewport fit or a fixed header. Existing window-drag limitations remain from the prior review. Appearance's layout symbols are conceptual choices, not calibrated native tray-icon assets.

No runtime/service, board task or installed settings changed; no commit, push or deployment. Cargo/runtime selftest were not run for this prototype-only iteration. Pinned growth previews, actual icon previews, complete settings taxonomy and the separate messenger remain subsequent design rounds.
