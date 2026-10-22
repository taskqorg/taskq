# Menu exploration: pre-presentation review

Exploratory revision reviewed on 2026-10-22 against shared D1-D14, F1-F6, taskq R6, visual-style targets and the owner's latest corrections. This is not product acceptance or approval of incidental prototype behavior.

- Source: [menu-session.html](menu-session.html).
- SHA256: `b9e10c59660c5aed3ed75f4bf112b1634e3bfd2d3e738cb04e0d48526e4582c0`.
- Fixture: six illustrative tasks; same filters/search in both panels, project groups versus flat ordering. Question #104 and failed #106 precede ordinary work in both modes.
- Inspected rendering: dark 16 px at 470 px per panel; light 20 px at 320 px per panel; full and single-result lists, repeated lifecycle stages, empty results and local detail/return.
- Evidence: visually inspected browser screenshots, accessibility snapshots, DOM measurements, actual UI actions and script syntax checks. The conversation display copy matches this source byte for byte.

| Rule / scope | Observed evidence | Result |
|---|---|---|
| Workflow: exploration | Session note records latest correction and open choices; approved product specifications and queue remain untouched by this revision | Pass |
| Workflow: comparison | Both modes visible together; no carousel; only grouping/context repetition differs | Pass |
| D4/D9/D11: repeated row | No task chevron or ellipsis; one compact forge/reference trigger, full-width title, state/elapsed value. Runtime, host and history moved to preview as an unapproved candidate | Pass for this comparison |
| D6 / R6: title geometry | Metadata first, wrapped title below. At 320/20, title width 302 px and indent 0 in both panels; first row 76 px high; no horizontal overflow. At 470/16, title width 452 px, row 70.398 px high | Pass for inspected fixtures |
| D3/D8: reference target and identity | Reference target 32 px high; approved repository GitHub asset rendered as a 15 px monochrome mask; accessible reference name includes project even when group context removes its repeated visible name | Pass for fine-pointer fixture |
| D7: text contrast | Main, muted and status text against normal surfaces calculated at 5.62:1 or greater in both palettes. Symbols and accessible state labels supplement color | Pass for named token pairs; all interactions not qualified |
| D8/F3: keyboard/action route | Actual Tab from search opens unpinned preview; Enter pins and focuses Task detail; detail opens with Back focused; Back restores reference/context; Escape closes; outside click in the other panel closes | Pass for exercised browser route |
| D8/F2: preview geometry | Clean 320/20 single-result fixture: 308 x 402.797 px preview, 3 px below anchor, pinned after click, no horizontal overflow, full preview inside expanded conversation surface | Pass for preview fixture; native work-area fitting not qualified |
| D13: attention | Question and failed task remain before ordinary grouped/flat rows. Long stage duration is not labelled stalled | Pass |
| Timing integrity | Repeated Doing/Review steps kept separately; #103 totals 22 min; failed step retains seconds rather than silently rounding them away | Pass for illustrative data |
| F3: continuity | TaskQ + Recover survives Settings/Back and browser reload; custom Review desk (TaskQ + Review) saved/pinned and restored after reload | Pass for local view state |
| Hover/pointer travel | Delayed hover opening and exit handling implemented, but actual pointer movement/transfer was not qualified: raw CDP input was denied by the approval gate. Keyboard/click evidence does not substitute for this | Not tested |
| Native/live effects | Native menu bounds, DPI, coarse pointers, screen readers, scrolling/restart journeys, live board/session routing and mutation receipts | Not tested |

## Defects corrected before presentation

- Removed the rejected below-icon menu cue and separate row action controls.
- The preview icon library lacked GitHub; reused the existing repository asset instead of a missing glyph or invented shape.
- Limited hover-dismiss handling to task previews so leaving an ordinary filter editor cannot dismiss it accidentally.
- Prevented focus restoration from re-entering disclosure during close; detail now starts at a reachable Back control. Final exercised routes produced no console errors.
- Used explicit dismissal for the interactive task preview; native automatic popover dismissal could race with focus opening and undo click pinning. Ordinary filter menus retain automatic popovers.
- Removed placement clamping that could cover the anchor. When neither side fits inside the short sample app, the preview goes below and the conversation surface expands. This is a preview adapter, not proof of native monitor sizing.
- Preserved state colors after the compact-stage selector initially overrode them.

## Open choices and boundaries

Total age and stage count are visible in the preview, not the current repeated-row candidate. Their removal from the list is not approved. Grouping is fixed differently per panel for simultaneous comparison; saving grouping/display preferences with each view remains a proposed next refinement.

Hover delays are prototype values, not established usability findings. Timing source, granularity, stale/unknown data and stall semantics remain unresolved. Header/settings and conversation links are illustrative, not a complete redesign or removal inventory. Native issue/session actions remain implementation gaps; no live task mutation, board task creation, commit, publication or deployment is claimed.
