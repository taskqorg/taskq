# Saved-view editor: exploratory review

## Owner correction and comparison re-review

The owner could not distinguish the alternatives in the delivered interaction. The earlier comparison-presentation Pass is withdrawn: shared rerenders and automatic popup dismissal erased the only visible difference. Functional filtering evidence did not establish that the comparison was usable.

Current source SHA256: `0a9beb58b8e870f25ef6fedbfac39070b4ad530376c40eeb2773664bf3b88370`. Top review controls now open or close both editors together; editing mode holds the popup open during interaction with the inline form and survives restored view state. These controls are outside product chrome and do not define application startup or dismissal behavior.

| Rechecked scenario | Observation | Result |
|---|---|---|
| Initial simultaneous comparison | Both forms visible. At dark 16 px / 470 px app, list begins at y=226.633 on the left and y=436.711 on the right: inline shift 210.078 px | Pass |
| Working mode | Both forms closed; both lists begin at y=226.633. Functional equivalence is explicit | Pass |
| Cross-panel interaction | Selecting a state in the right form leaves the left popup visible | Pass |
| Apply and reopen | Apply closes both forms and shows #103/#105; top control reopens both; browser reload restores active criteria with both editors visible | Pass |
| Large text / narrow | Light 20 px / 320 px apps: both forms visible, no horizontal overflow; list y=261.984 versus y=614.969 | Pass |
| Runtime / product boundaries | Inspected browser route produced no console errors. Native menu, actual hover travel and full keyboard traversal remain unqualified | Not tested as product acceptance |

Screenshots for both themes were visually inspected after this correction. No product choice or specification approval was inferred. The checks below describe the previous source revision and retain their historical scope.

## Previous revision evidence

2026-10-22. Source: [view-editor.html](view-editor.html). SHA256: `2357434a8582ece5673ea24d904cc9e5213109464bbbfa85649f093f068fae62`.

Scope: popup versus inline editor placement, using the prior six-task fixture and unchanged row/preview composition. Both panels share criteria/search for comparison. Initial open editors demonstrate the alternatives; this is not an approved application startup behavior. Product requirements remain unchanged.

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow: comparison and phase | Both alternatives visible together; same fields, sequence, tasks and shared state; placement is the disputed decision | Pass |
| D2/D3/D6: grid and text | Dark 16 px, 470 px app: popup columns 158.5/158.5 px, inline columns 219.5/219.5 px. Light 20 px, 320 px app: single-column forms 292/296 px, title width 302 px; no horizontal overflow | Pass for inspected fixtures |
| D4/D11: normal working view | Editor disappears after Apply/Cancel. Save-copy stays in view menu; temporary changes expose one Save action and a compact restore control. Edit stays at the bar's upper-right while pinned names wrap | Pass for inspected fixtures |
| D8/F2: overlay placement | At 320/20 after layout settles, popup begins 3 px below Edit. Placement now remeasures after icon hydration and app resizing; the preview surface grows around overlays | Pass for browser fixture; native monitor fitting remains open |
| D8: editing | Popup Apply selected Doing + Review, stage-age sort and no grouping; both panels showed #105 then #103. Inline Apply/Save persisted project grouping; Cancel did not apply a temporary Failed selection | Pass |
| D8/F3: copy/save/restore | Created Active agents and Active work through Save as; saved state/grouping survived browser reload. Changed sorting and restored the saved criteria through Revert | Pass for local prototype |
| F3: working context | All tasks temporary Active/None criteria and Keep search survived Attention/All tasks and Settings/Back; only #105 remained. Search is per-view working context, separate from saved criteria | Pass for exercised routes |
| F3: focus | Apply/Cancel return toward Edit; Revert focuses the current-view button. Identical host state echoes no longer rerender the surface and replace focused controls | Implemented; Apply/Cancel/restore observed, full keyboard journey not qualified |
| D13: attention | Existing attention-first partition retained; Active means Doing plus Review, not a new task lifecycle state | Pass for fixture and model checks |
| Model validation | Isolated executable checks cover workspace restoration, invalid grouping rejection, active filtering and reverting to saved criteria; script syntax checks pass | Pass |
| Runtime errors | Final inspected dark browser route reported no console errors | Pass for inspected route |
| Native/live behavior | OS menu bounds, screen readers, touch, full keyboard traversal, native refresh/scroll/restart, real task actions, actual hover travel | Not tested |

Visual checks inspected open forms in both themes and closed/dirty/saved views at large text. Corrections before presentation: limited forms to two consistent columns or one when narrow; kept Edit out of an otherwise empty wrapped row; removed the duplicate Save as action from the condition row; avoided identical state-echo rerenders; remeasured overlays after layout changes.

No canonical product decision, application runtime change, board task, commit, push or deployment is included. Preset names/defaults, temporary-state retention policy, editing placement and application persistence remain proposals. Total task age in the repeated row is still unresolved from the preceding round.
