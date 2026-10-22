# Connected control panels: exploratory review

Source: [control-panels.html](control-panels.html). SHA256: `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`.

Current revision: Conversations closing walkthrough. Earlier unchanged-component evidence below comes from baseline `f567bdd109b375b5a6c5b54f53b25ede1cca5e47964bcff9476c0a43d8253cca`; revised-route evidence follows it. Exact browser revision marker: `conversation-walkthrough`.

Scope: connected design candidate for menu/settings and Conversations. Reuses the accepted task-selection/window reference plus existing AI, custom-icon and agent-settings components. C2 adds the owner-requested operator-window treatment. Exact section grouping, controls and styling remain exploratory. All data/actions are local fixtures; no native policy, tray or service changed.

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow / comparison | Menu and workspace visible together; review selectors above both. Default desktop frames580px high, no carousel | Pass |
| D3/D6/D9 | Final dark16 desktop inspected: metadata then wrapped task title; panel client/scroll widths358/358 and766/766. Light20 constrained workspace302/302, no horizontal overflow | Pass for inspected fixtures |
| D4/D11 / icons | Current/draft tray choices use the shared22px renderer. Optional16/32px and labelled ×4 inspection collapsed; resize strokes integrated into the corner | Pass for inspected fixtures |
| MP11–13 / continuity | Doing + TaskQ search retained through Notifications, local preference change and return; All scoped count4, matching task107. Reload retains filter and query | Pass in browser |
| C-A1 / drafts | Admin API and Release checks retain different drafts across selection; Admin draft remains after reload. Running request permits drafting but disables Send | Pass in browser |
| C2/C7 / management | New → rename UI review → draft → Archive → Restore → Send reaches Queued with draft cleared and Send/Archive blocked. No request sent externally | Pass in browser |
| C2 / window | Desktop Enlarge hides adjacent menu, Restore returns it; workspace ArrowDown changes580→596px. Final narrow Enlarge works in one click; composer Send remains right-aligned and footer visible | Pass for exercised browser paths |
| C2 / narrow navigation | List replaces transcript; selecting a conversation returns to content. Separate list and transcript inspected at large type | Pass in browser |
| CI1/CI3 / icon editing | Graph preset → Use icon marks Custom in menu; built-in Summary remains separate. Shared renderer used for both | Pass in browser |
| AS2/AS3 / access | Read only changes Codex draft; Review shows Workspace → Read only. Reload restores the review draft; no actual grant applied | Pass in browser |
| State integrity | Source-derived store test saves menu, chat and icon sequentially without clobbering; oversized update refuses while preserving the last snapshot | Pass |
| Syntax / source | Seven scripts parse; no unresolved config tokens; single host-state writer; display/source equality; root contract remains300 lines/61400 bytes; diff whitespace check passes | Pass |
| C-A4–9 / full adapters | Attachment selection/error/byte recovery, stale confirmation, partial writes, all provider errors and native session targets | Not requalified |
| Native acceptance | Tray slot/DPI/font raster, live window drag/resize/blur, separate detached owners, real Pin/topmost, monitor changes and screen reader/IME | Not tested |

Corrections before presentation: removed repeated review buttons; moved repeated Registered text into project context; represented one host service instead of per-project services; moved icon inspection sizes out of the default editing flow; fixed narrow composer wrapping; retained independent preview-frame positions. The adjacent workspace is exempt from simulated popover blur only in this comparison, preventing a moving click target; this does not change M13 native semantics. Closing a window never archives/cancels or clears the draft.

Coverage limits: Work mode covers host Auto/Fixed and maximum, not full MP2 scope matching/rule-vs-snapshot review. Service operations and native file/update actions remain disabled with reasons. New archive-search persistence, saved geometry and Escape handling were source-checked; those final refinements do not claim complete native qualification. Unsaved Custom edits remain in the mounted editor through navigation; reload recovery of that draft remains open. Exact notification delivery, settings taxonomy, stage-history density and total-age visibility await review.

No cargo/runtime selftest, board mutation, native installation, commit, push or deployment. This is a walkthrough candidate, not a complete production acceptance package.

## Conversations closing walkthrough

| Finding / rule | Baseline → current candidate | Evidence / result |
|---|---|---|
| CW1 / D5 creation | New required a second click in the composer and inherited a search hiding the new row → New clears search, selects the visible row and focuses the composer | Search Release → New: empty search, selected new row, focused textarea; no Editing/helper outcome. Pass |
| CW2 / C-A1 draft return | Draft had a generic title and needed another composer click → first draft line identifies it; selecting an editable draft focuses its text | Design draft → Release checks → separate release draft → Design draft: original text and focus retained. Explicit titles are unchanged. Pass |
| CW3 / C2 archive | Archive jumped the entire list into Archive → stay in current list, select remaining visible item, offer Undo | Archive selected design draft: Archive count2, Release checks selected, Undo visible. Undo restores original draft/focus. Pass |
| CW4 / C7 persistence | Chat-first snapshot contained undefined activeView/trayView; save refused and reload lost the draft → JSON-safe snapshot with absent summary fields omitted | Before: saving-unavailable state and lost new draft after reload. After: chat-only workflow then reload restores design/release drafts; no menu setup needed. Pass |
| CW5 / narrow direct action | From transcript: List → New → composer click → direct New with focus | Light20, workspace302px client/scroll width: New visible beside List, immediate focus, switch via List and return retains Narrow draft. Pass |
| CW6 / context | Archive/Recent navigation forgot the previous selection; clear-search could select an unrelated row → remember selection per list, preserve current selection on clear | On exact revision marker, Archive → Recent restores Design draft; no-match search → Clear search keeps Design draft. Pass |
| CW7 / D4 wording | Repeated Editing/helper text and Needs answer row/group words → remove generic empty outcome, use group/state symbol and concise draft metadata | Dark16 and light20 renders inspected. History deletion remains under History; source-numbered answers and unavailable-state explanations remain. Pass |
| CW8 / recovery | Close/reopen must neither submit nor lose the draft | Close workspace → reopen: original text and focus. Explicit Archive → select design draft → Restore returns its text. Pass. Latest Escape tool action timed out; not requalified |

Desktop clicks, excluding typing: New/start writing2→1; return to a visible draft2→1; Archive remains one action and no longer forces a list-view return. In a narrow transcript, New/start writing3→1; switching to an existing conversation still uses List then its row. No new shortcut vocabulary or per-row action column is introduced.

Regression command: `node docs/design/exploration/control-panels-check.cjs` — Pass. It parses all scripts, rejects undefined anywhere in the first chat snapshot, checks menu/chat isolation and an empty selection, refuses an oversized write without replacing the last state, and distinguishes a temporary draft title from an explicit title. Display/source equality and whitespace check pass. The browser save API now accepts chat-first state; prior fake-writer tests alone missed that boundary.

The owner subsequently accepted the overall closing result and requested final packaging and PM decomposition. These refinements are the frozen closing reference; bounded open policies remain in the implementation brief. No delivery/cancellation/attachment adapter, native window ownership, permissions, queue work or production behavior is qualified by this review. The final brief records the subsequent PM handoff/task-creation authority; no implementation starts under this browser review.
