# TaskQ visual system

Final candidate for the approved dense menu. Shared principles: [specq D1–D14](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md).

## Geometry and type

- System UI font: SF Pro on macOS; Segoe UI Variable/Segoe UI on Windows. Normal shapes, weights 400/500.
- Menu base 16 px; user options 14/16/18/20. Secondary text at least max(12 px, 0.85×base).
- Native width and content-fit height follow [R-UI2](rendering-contract.md#r-ui2-native-menu-sizing); preview width is 360 px. Fixed demo heights are not production rules. Never scale the entire window down.
- Outer inset 8 px; row horizontal inset 4 px; related gap 4–6 px; section separation 6 px with one 1 px divider.
- Approved control hit targets follow [UI05–UI06](ui-rules.md#checklist-28-rules): at least 32×32 CSS px for fine pointers and 44×44 CSS px for coarse pointers; native units require scaling verification. Icons normally 16 px; compact top-strip icons keep usable targets.
- Menu radius 12 px; controls 5–7 px; counter capsules; counter symbols may use 14 px within 32 px targets. No nested decorative cards; bordered surfaces only for composers and consequential change review.
- Header/control strip stays accessible. Native height is bounded by the monitor work area; scroll content, never clip it. AI window keeps a stable heading and one reading viewport ([D4](ai-workspace.md#workspace-dialogue-d1d8)); Send stays reachable, short input stays at the bottom and a long draft grows into that viewport, never into an inner text scroll.
- At short/narrow sizes, reflow header controls or use the conversation list as a separate page. Maintain Back, Send, context and attachments.
- UX07: selected Conversations threads show a plain heading, one collapsed Details control, reading content and a concise composer. Details groups secondary controls without nested watch/launch disclosures. Hide unsupported Attach; keep limitations in Details and actionable validation beside the input. Preserve the palette, icon catalog, type scale and 32/44 px targets.
- v7 window (#1773, adopted from the [preservation candidate](exploration/conversations-preservation/candidate.css)): an entry bar (New chat in the work color, Search threads and Work sessions icons) over the list; Recent/Archive icon tabs and a search line; the selected thread spans the full height. The thread head holds the title, window and archive icons and one PM status line (PM is replying, PM is ready, PM status unknown). Owner messages are raised "You" bubbles, replies are "Project manager" text and a launch brief collapses to Setup brief. The composer is one bordered box: project line above, text, then a tool row with Attach (paperclip), New from draft (file) and History icons left and an arrow Send right. A task thread shows a task card with its state. Mark read is an icon. Only Lucide icons from the catalog and icon.rs palette tokens (applied to the workspace window too).
- Metadata starts the task's first line; title continuation starts at the full block left edge. Do not shrink the title to accommodate columns.

## Palette

Palette A follows the [approved icon review](../reviews/visual-decisions.md). Runtime semantic tokens come from `src/icon.rs`; menu components inherit them; independent exported components retain their standalone defaults.

| Token | Light | Dark |
|---|---|---|
| Surface | #FAFBFC | #252B32 |
| Text | #243039 | #EEF2F5 |
| Secondary | #59636E | #B9C3CD |
| Divider | #DCE0E5 | #46505B |
| Hover/selected surface | #E9EDF1 | #37424C |
| Work / closed | #087F5B | #38E8A0 |
| Question / confirmation | #995200 | #FFC247 |
| Review / awaiting merge | #8250DF | #BCA3EB |
| Neutral: Ready / Blocked / Later / Held | #536171 | #B9C6D8 |
| Closed text / fill | #68727D / #EFF1F3 | #9BA5B0 / #2B3138 |
| Failed | #C92641 | #FF6378 |

The owner accepts the purple Review/awaiting-merge accent. Light #8250DF follows [Primer's done foreground](https://primer.style/product/primitives/color/); dark #BCA3EB is adapted to TaskQ's existing palette, not an asserted GitHub token. GitHub uses done for completed issues/merged PRs; TaskQ's waiting accent is its own convention. A confirmed merged PR uses a distinct merge symbol and explicit result label; purple alone never asserts publication, merge or completion. Do not recolor ordinary closed-history rows or add a merge lifecycle state.

Use subtle counter fills, not colored text blocks. Always include a symbol or label. Board-label HEX values remain in board.md; do not replace them with UI tokens.
Closed has the lowest visual emphasis without opacity; [counter semantics](task-counters.md).
Paused data is gray; partial pause/unknown does not gray all counts. Unknown ([never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy)) is labelled.

## Icon inventory

Use one outline family for actions, consistent 16 px optical size. Reuse the existing bundled Lucide static 1.17.0 catalog (ISC, with retained Feather MIT notices), 24-unit viewBox and source stroke width 2. Render the same SVG bytes on Mac and Windows; no Unicode, SF Symbols or platform font substitution. New assets must come from that pinned catalog, retain license/source metadata and be reviewed before use. Provider/forge marks remain separate licensed identity assets.

### Task-state icons

The owner accepts the mapping below and the paired theme colors on 2026-10-23, reusing the implementation's existing glyphs. It supersedes accidental symbols in editable drafts; it does not silently amend the frozen reference. Use one mapping for counters, filters, rows, details and Settings project summaries. Preserve labels/tooltips and accessible state names.

| Meaning | One icon | Accent |
|---|---|---|
| Doing | play | Work |
| Review / awaiting merge | git-pull-request | Review purple |
| Ask | message-circle-question | Question |
| Failed | circle-x | Failed |
| Ready | circle | Neutral |
| Blocked by a dependency | link | Neutral |
| Later | clock | Neutral |
| Held | circle-pause | Neutral |
| Confirmed merged PR result | git-merge | Review purple; explicit Merged label |

Blocked's link means dependency; never use a pull-request or merge glyph for it. Actual blocking references/reasons belong in details. Held is a task state; the admission Pause control is a different action. Review does not prove a reviewer is currently running. Loading indicators describe loading, never Doing. Missing asset names must fail catalog validation; never silently substitute a circle that looks like Ready. `git-merge` can also identify explicit merge policy/actions, but never asserts that they have completed. It is absent from the inspected runtime bundle; implementation must add the pinned asset before using that route.

| Meaning | Icon name / asset |
|---|---|
| Admission | play / pause |
| Controls / Work mode | sliders-horizontal / focus |
| Attention/question/failure | circle-alert / message-circle-question / circle-x |
| Project / pin / add | folder / pin / plus |
| Board / issue | github.svg or gitlab.svg plus external-link where needed |
| Conversations / Work sessions | messages-square / terminal |
| Attach / send / history | paperclip / arrow-up / history |
| Detach / return / close | panels-top-left / panel-left-close / x |
| Search / archive / restore | search / archive / archive-restore |
| Runtime | claude.svg; openai.svg with explicit Codex label, including codex-app |
| Remote host | network; no local-session link |
| Log / folder / restart | file-text / folder-open / rotate-cw |
| Stop / uninstall | square / trash-2 |
| Appearance / cadence / agents / services / about | type / chart-no-axes-column / bot / activity / info |

Icon-only buttons have accessible names, tooltips and focus rings. Brand marks identify the provider; visible/accessible labels identify the exact runtime. Never invent a permalink from the mark.

## Tray assets

- Reuse the accepted compact q geometry. `assets/q-*.png` are installed 4.1.8 fixture exports for reference, not universal production sizes.
- Summary follows [I1–I5](icon-summary.md). Do not copy the old wide strip or segmented digits.
- Active summary: fixed square with three counters and graph. Paused: identical gray data with compact q/pause centered.
- Overflow, unknown rendering, 6/12 bins and Windows 16 px still need explicit acceptance. The candidate does not silently choose a lossy policy.

## Asset sources

`claude.svg`, `openai.svg`, `github.svg`, `gitlab.svg`: Simple Icons 15.0.0, https://cdn.jsdelivr.net/npm/simple-icons@15.0.0/icons/ (CC0 library; marks retain their owners' trademark rights). OpenAI mark is provider identity, not a claimed separate Codex logo.
Other icons: the bundled Lucide static 1.17.0 catalog is the source. Historical prototypes use the preview renderer's Lucide and are not byte-level asset references. The icon review sheet reuses bundled SVG bytes; remaining prototype-only names must be reconciled with the pinned catalog before implementation. No production CDN dependency is required.
The review bundle retains [Lucide/Feather notices](exploration/assets/icon-review.LICENSE). Its additional comparison and merge vectors come from `https://cdn.jsdelivr.net/npm/lucide-static@1.17.0/icons/`; provider marks retain the source metadata above.

## Revision contract

- The frozen `exploration/control-panels.html` reference in the implementation brief owns unaffected geometry, icon shapes and semantic colors. A new prototype must reuse these tokens/assets; a replacement theme or symbol is a product change, never a renderer convenience.
- Task separators use a separate low-emphasis token: Text at 8% over Surface. Do not weaken control boundaries with the same token. Popovers use an opaque raised surface (#FFFFFF / #323B45), a distinct edge (#8996A2 / #8996A2) and shadow, above the list without reflow. Check both themes; stronger separation must not mean neon semantic colors.
- Keep forge/issue identity as the compact clickable anchor; no new boxed status column, right ellipsis or disclosure arrow. Runtime marks and the remote network mark sit immediately before current-stage timing. Titles wrap from the full block edge.
- Collapsed host summaries use one compact line with named hosts and separate Claude/Codex counters. CPU/memory mini-meters with units appear on expansion. Missing or stale values remain explicit; never infer host health or progress from task count. Detail discloses definitions and freshness.
- Text entry, attachment chips, validation, context and send controls share one composer across every applicable surface. A task answer retains source numbers and exact-revision semantics; visual reuse does not grant file-answer transport or native reply support.
- Before presenting a revision, compare the whole affected menu/workspace against the pinned reference, list intentional differences against requirement IDs, inspect rendering and exercise navigation/state retention. Record Pass / Fail / Not tested in the existing revision review. Owner review does not replace this gate.

Windows and pins: [P-W1–P-W3](menu-product.md#windows-and-task-pins) use one custom chrome component for TaskQ-owned windows. Keep titles/drag regions and window controls consistent; do not introduce a system titlebar on Conversations or reuse the same pin action for unrelated effects.
