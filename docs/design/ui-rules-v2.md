# App UI rules v2 — proposal

Owner review for [#1442](https://github.com/taskqorg/taskq/issues/1442).
This proposal extends [approved UI01–UI28](ui-rules.md#checklist-28-rules)
and [shared D1–D14][D]; neither is replaced before approval (R13).
It covers new tray, workspace and DashQ screens as a design target, not shipped
behavior, WCAG conformance or a grant of runtime authority (R8/R9/R12).
TaskQ-specific values and routes remain here even if the generic checks move upstream.

## Sources and interpretation

Read 2026-10-21. Apple’s navigation-bars URL redirects to Toolbars; its official
Markdown, including iOS and macOS sections, was read for each of A1–A5.
Material’s direct pages returned JavaScript shells; indexed primary-page text
was readable for M1–M3. G supplies the separately read official Android target
recommendation. No third-party digest substitutes for a primary source.

| Key | Primary source and relevant scope |
|---|---|
| A1 | [Apple toolbars/navigation bars](https://developer.apple.com/design/human-interface-guidelines/toolbars), [Markdown](https://developer.apple.com/design/human-interface-guidelines/toolbars.md): navigation, title/action groups; iOS large titles and macOS window toolbar/menu commands |
| A2 | [Apple sheets](https://developer.apple.com/design/human-interface-guidelines/sheets), [Markdown](https://developer.apple.com/design/human-interface-guidelines/sheets.md): scoped tasks, dismissal; modal macOS sheets versus modal/nonmodal iOS sheets |
| A3 | [Apple lists and tables](https://developer.apple.com/design/human-interface-guidelines/lists-and-tables), [Markdown](https://developer.apple.com/design/human-interface-guidelines/lists-and-tables.md): selection, readable rows; iOS disclosure versus macOS sortable/resizable columns |
| A4 | [Apple layout](https://developer.apple.com/design/human-interface-guidelines/layout), [Markdown](https://developer.apple.com/design/human-interface-guidelines/layout.md): hierarchy, adaptation and safe areas, including iOS size classes and macOS window constraints |
| A5 | [Apple typography](https://developer.apple.com/design/human-interface-guidelines/typography), [Markdown](https://developer.apple.com/design/human-interface-guidelines/typography.md): readable type; iOS Dynamic Type, macOS system fonts without Dynamic Type |
| M1 | [Material 3 app bars](https://m3.material.io/components/app-bars/): page identity, navigation and essential actions |
| M2 | [Material 3 applying type](https://m3.material.io/styles/typography/applying-type): semantic roles and platform-sensitive typesetting |
| M3 | [Material 3 motion](https://m3.material.io/styles/motion/overview/how-it-works): standard versus expressive motion schemes |
| G | [Google Android touch targets](https://support.google.com/accessibility/android/answer/7101858?hl=en-GB): 48 dp targets, distinct visual and input bounds |
| F1 | [Fluent 2 layout](https://fluent2.microsoft.design/layout): proximity, four-unit ramp with 2/6/10 exceptions, adaptive grids, platform units |
| F2 | [Fluent 2 typography](https://fluent2.microsoft.design/typography): native fonts and platform-specific type ramps |
| F3 | [Fluent 2 dialogs](https://fluent2.microsoft.design/components/web/react/core/dialog/usage): focused interruptions, persistent controls, focus restoration |
| W | [WCAG 2.2](https://www.w3.org/TR/2024/REC-WCAG22-20241212/): criterion numbers below identify the exact checks; not a navigation or visual-style prescription |
| N | [Nielsen’s ten heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/): N1 status, N2 familiar language, N3 control, N4 consistency, N5 prevention, N6 recognition, N7 efficiency, N8 minimalism, N9 recovery, N10 help |

Each checklist row cites applicable sources and records **Agree** (shared intent,
not identical measurements) or **Conflict** (a different recommendation or local
choice). Unlisted sources do not prescribe that detail; silence is not consensus.
Exact TaskQ dimensions/filters come from UI01–UI28 and [visual-style.md](visual-style.md),
not from external standards. CSS px, Apple pt, Android dp and physical pixels
are different units; native ports need their own qualification.

## Navigation model for review

This is a proposed logical route tree, not a claim that URL handlers exist.
It preserves current `src/tray-app.js` routes and the accepted workspace entry.
Logical identifiers below map to URLs only through a consumer’s route ledger.

```text
Home (root)
├─ Attention
├─ Projects
│  └─ Project {board, project-id}
│     └─ Tasks {selected states} (counter shortcut or Filter)
├─ Settings
│  ├─ Appearance
│  ├─ Activity history (existing cadence route)
│  ├─ Notifications
│  ├─ Agents
│  ├─ Services
│  └─ About
├─ Activity history (same destination as Settings → Activity history)
├─ Work mode {selected projects} (existing view, entry context retained)
├─ Stop and hold (review before apply)
└─ Reviewed recovery (review before apply)
Workspace window (own root, existing window policy)
├─ AI conversations → Conversation {id}
├─ Work sessions → Session {id}
└─ Agent settings {host, optional project}
External destinations: Board, Issue, native Session, PR; open directly.
```

Home/Settings shortcuts push from the current view; only explicit Home-root
entry has no history. Every non-root route shows Back, including Settings and
Attention; deep entry without history uses its logical parent. A workspace
root has no synthetic Back to a different window; its own child routes do.
A shared Activity destination keeps the actual caller as Back, not a fixed parent.
Switching windows does not destroy either window’s route or drafts.

Header slots: leading Back (reserved at root), one route title next, trailing
context actions followed by a separate window-control group. Native title-bar
controls remain OS-owned; browser surfaces do not invent native window controls.
At narrow widths, secondary actions move to an accessible overflow; Back and
exit remain reachable. Emergency stop and window Close stay distinct. Leading
and trailing follow reading direction. No huge title block is required.

Push for browsing, settings and long editing flows; a modal sheet/dialog only
for a short scoped decision or input. A modal has Cancel/Close and an explicit
commit action when relevant; Back inside it moves one step, never dismisses it.
No nested modal. Dismissal never implies apply or successful cancellation of an
already submitted operation. Preserve drafts or review their loss before leaving.

Web consumers encode the route and shareable filters in their existing URL
format; native consumers map the same route identity to their supported link
handler. No new URL scheme is introduced here. Encode stable board/project ids,
not host filesystem paths; never secrets or private drafts. Direct entry/reload
resolves the requested view under current access. Bad, removed or unauthorized
destinations show an explicit reason and a valid recovery link; no silent Home
fallback. Browser Back/Forward restores history; native Back restores its stack.
A future DashQ consumer keeps its own entity tree and URLs, mapping these generic
rules before adoption; this proposal does not silently prescribe its product routes.

Return restores filters, expanded groups, scroll, selection and focus to the
originating control; if it disappeared, focus the nearest surviving item or list
heading. First entry focuses the route heading; refresh preserves the current
route/draft/focus. Persist route identity across reload, not modal execution or
unconfirmed operations. Changed/unavailable data is revalidated and explained.
These are proposed acceptance checks, not evidence of current persistence.

## Checklist: 38 checks

V IDs are proposed aliases; historical UI and D identifiers remain valid.
The navigation model above supplies the concrete expectations for V01–V07.
Each row is one review check with an observable pass/fail boundary.

| ID | Testable check | Source agreement or conflict |
|---|---|---|
| V01 | Every exposed route/control maps to a destination in the consumer route ledger, including empty, unavailable and unauthorized cases; no unexplained dead end. | Agree A1, N2/N6; local route tree, not a vendor-prescribed tree. |
| V02 | Back follows the navigation model on every child route and shifts by ≤1 CSS px during scrolling; roots do not offer a fictitious previous view. | Agree A1, N3/N4; UI01 retained. |
| V03 | Header slots remain ordered and hit rectangles never overlap at 320/360 px and 14/16/18/20 px, including overflow and all window actions. | Agree A1/M1/F1; Conflict A1 iOS large-title preference: compact TaskQ title; native macOS chrome remains native. |
| V04 | Browsing uses push; a short modal flow returns to its invoker through Cancel/Close without applying; Back only changes a step; drafts survive or receive a loss review. | Agree A2/F3/N3; Conflict A2 permits nonmodal iOS sheets: use an explicitly nonmodal helper for that case, not a falsely modal dialog. |
| V05 | Direct link, reload, shared filtered view and browser Back/Forward resolve the same authorized route/state, or show an explicit unavailable reason and recovery destination. | Agree N4/N6 and [shared F7][F]; exact URL format stays consumer-owned. |
| V06 | List → detail → Back restores filters, expansion, selection, scroll and originating focus, with the documented missing-item fallback. | Agree A3/F3/N3; UI02 plus shared F3. |
| V07 | A snapshot refresh preserves route, local drafts and focused control; open views never reorder their active targets without explicit navigation. | Agree N4 and D stable adaptation; preservation is a product choice. |
| V08 | Board is beside project identity; issue, session and PR links each open the named target independently without also activating their row. | Agree D5/D8, N7; TaskQ UI04/M3 defines destinations. |
| V09 | Actual hit boxes measure ≥32×32 CSS px for fine pointers and ≥44×44 for coarse pointers, without overlapping adjacent targets. | Conflict G Android 48 dp versus TaskQ UI05 32/44 CSS px; F1 also distinguishes web/iOS 44 from Android 48. Preserve local values, no unit conversion claim. |
| V10 | Every smaller target documents its applicable WCAG 2.5.8 exception and separately records any unmet TaskQ V09 target; an AA exception never silently waives UI05. | Agree W2.5.8; Conflict universal 44/48 claims: AA is 24 CSS px with spacing/equivalent/inline/user-agent/essential exceptions. |
| V11 | Every icon action exposes the correct accessible name, tooltip and visible focus indicator. | Agree A1/D8, W4.1.2; UI07 retained, tooltips do not replace names. |
| V12 | Keyboard-only traversal reaches and activates every enabled action with the control’s standard keys and can exit every interaction. | Agree W2.1.1/2.1.2, N7; UI08 retained. |
| V13 | Focus follows reading order and stays visibly exposed through navigation, scrolling, validation and refresh. | Agree W2.4.3/2.4.7/2.4.11, F3; UI09 retained. |
| V14 | Opening a modal moves focus inside, contains traversal, exposes a named exit and restores invoker focus on dismissal; other windows remain usable for a macOS sheet. | Agree F3/A2; Conflict one app-wide focus trap: macOS sheets scope to their parent window. |
| V15 | Consequential review names target/effect before confirmation and visibly distinguishes saved, pending and readback-applied. | Agree F3/N5, TaskQ M5/UI10; modal styling never grants authority. |
| V16 | Each repeated identity, destination, counter, card or label has distinct context; removing duplicates/gaps precedes any size reduction and never shrinks text for density. | Agree D4/D9, N8; Conflict F1 roomy rhythm versus operator density: retain meaningful grouping, remove decorative space. |
| V17 | Empty summary groups occupy zero rows and decisions/faults precede ordinary lists, ahead of completed conversations/maintenance; directly opened empty destinations explain their emptiness. | Agree D13, A4/N1; local UI12 priority, not a universal vendor ordering. |
| V18 | At 320/360 px with 180-character titles, status/issue/title begin one flowing block, continuation uses full width, and identifiers remain intact without collisions. | Agree D1/D6, A3; Conflict rigid/truncated list templates: retain TaskQ full-width wrapping. |
| V19 | Counters retain order, semantic symbols and group placement for 0/1/12/100/1000 and partial data; zero leaves no menu capsule and unknown stays labelled. | Agree D10/N1; TaskQ counter policy; summary-icon fixed zero slots remain an explicit exception. |
| V20 | Tray geometry matches outer 8 px, row-horizontal 4 px, related 4–6 px and section 6 px plus one 1 px divider; other surfaces declare their spacing tokens. | Agree F1’s four-unit ramp permits 6; Conflict uniform 8-only grids: local UI15 remains. Do not impose tray insets on DashQ. |
| V21 | Names align left and compact counter groups align right on reused visible edges; changing count width neither collides nor expands unrelated gaps. | Agree D2/D10, F1/A4; local UI16. |
| V22 | Type follows the role scale below at every selected base size, including secondary text; tray font persists and summary-icon size stays independent. | Agree A5/M2/F2 semantic hierarchy; Conflict copying native/default ramps: use TaskQ UI17 values; macOS has no Dynamic Type. |
| V23 | 200% text resize preserves content/functions and the 320 CSS px reflow check has no ordinary-text horizontal scrolling. | Agree W1.4.4/1.4.10; UI18 retained, not a blanket exception for whole pages with a data table. |
| V24 | Composited text contrast is ≥4.5:1 in both themes and meaning survives without color. | Agree D7/W1.4.3/1.4.1; Conflict W allows 3:1 large text: stronger TaskQ UI19 remains. |
| V25 | Essential control/state graphics meet 3:1 adjacent contrast, with each applicable WCAG exception recorded. | Agree W1.4.11; UI20 retained. |
| V26 | Natural content fits when space permits; overflow has one intended primary scroller and a reachable final control, with fixed navigation outside it. | Agree F3/A4 and shared F2/F3; local UI21/native available-area rules, not fixed viewport height. |
| V27 | Normal, loading, empty, no-match, stale, unknown, partial and failed fixtures produce distinguishable states with a reason/recovery where applicable. | Agree N1/N9, UI22/R12; no invented timeout changes the backend state. |
| V28 | Every effect displays pending, readback success, failure or uncertain outcome, and retry/reconcile never silently duplicates an effect. | Agree N1/N5/N9, UI23/R4/M5; navigation/dismissal is not a receipt. |
| V29 | Non-focus status updates are announced programmatically without moving focus. | Agree W4.1.3, UI24; routine refresh is not an urgent alert. |
| V30 | Filter starts closed; summary names selected states or All even with zero selections, task list follows header, independent checkboxes toggle with Space without closing; no usage hint. | Agree N6/W predictable input; exact UI25 disclosure semantics are local, not mandated by WCAG. |
| V31 | Select-all visual/accessibility state agrees for zero/all/some as unchecked/checked/mixed. | Agree [WAI checkbox pattern][C], N4; UI26 retained. |
| V32 | Fixture matches equal state OR intersected with selected projects; zero selected states imposes no state restriction and Reset restores all. | Agree N4 predictable behavior; exact UI27 set operations are TaskQ choices, not vendor rules. |
| V33 | Filters remain view-only, show selection/match count, survive Back, offer Reset on zero matches and preserve existing single-state counter shortcuts without queue/focus writes. | Agree N3/N4, UI28/M4/R9; selection is not admission authority. |
| V34 | Add is beside its section heading, external link beside its entity and small actions beside their subjects; direct primary actions need no intermediate link-only screen; settings group by purpose with recognizable icons and labels. | Agree D5/D11/D14, A1/N7; compact glyphs still pass V09–V13. |
| V35 | Reduced-motion mode removes nonessential spatial animation; updates keep targets stable and no essential state relies on motion. | Agree [Apple motion accessibility][AM], M3 standard utilitarian scheme; Conflict expressive bounce as default: operator transitions stay functional. |
| V36 | Essential control locations stay stable in normal, blocked, critical and frequent-use fixtures; only consequence or explicit preference changes content priority. | Agree D stable adaptation, N4; Conflict opaque frequency-based rearrangement: no automatic target movement. |
| V37 | The adopted checklist stays ≤40 checks and links canonical rules rather than copying them; every local exception names its reason and owner decision. | Agree D12 and shared documentation; local review budget, not an external standard. |
| V38 | One representative component’s complete list/detail/action/result/error/return flow is inspected at actual size with long text, larger type, narrow width and both themes before reuse. | Agree D review, A4/A5 context testing; R12 keeps model/browser/native evidence separate. |

Type scale for V22: base **b = 14/16/18/20 CSS px**, default 16, from UI17;
body/action labels **b**, secondary **≥max(12 px, 0.85b)**, following [existing geometry](visual-style.md#geometry-and-type).
Proposed additions: section headings **b semibold**, route title **1.1b semibold**.
These two heading roles are local v2 proposals, not previously approved geometry.
Use the platform system font. This is a small role scale, not Material’s full
15-role ramp or Fluent’s platform defaults. Do not downsize labels to make a
header fit. Native iOS consumers use scalable semantic styles and test Dynamic
Type; this does not claim macOS supports it. The summary icon keeps independent
calibration; meaningful in-content symbols must remain legible at larger text sizes.

[D]: https://github.com/taskqorg/specq/blob/d496acf9365edfe1c11132f363ae5aaa7573e1f6/design/operator-ui.md
[C]: https://www.w3.org/WAI/ARIA/apg/patterns/checkbox/
[AM]: https://developer.apple.com/design/human-interface-guidelines/accessibility
[F]: https://github.com/taskqorg/specq/blob/d496acf9365edfe1c11132f363ae5aaa7573e1f6/design/implementation-fidelity.md#f7-stable-view-urls

## Preservation map

Kept = same protection; merged = retained within a combined check. None deleted
or weakened. This table is traceability, not another checklist or adoption of D
outside its canonical home. UI01 is clarified for workspace roots/deep entry,
not waived on any existing tray child route. New detail remains pending approval.

| Existing ID | Disposition → v2 | Reason |
|---|---|---|
| UI01 | Kept → V02 | Stable Back; explicit root/deep-entry semantics. |
| UI02 | Kept → V06/V07 | Return and refresh protection, extended to focus/drafts. |
| UI03 | Kept → V03 | Same widths/fonts and collision check. |
| UI04 | Kept → V08 | Independent direct destinations. |
| UI05 | Kept → V09 | Approved 32/44 distinction, no false universal minimum. |
| UI06 | Kept → V10 | Exceptions documented without weakening the product target. |
| UI07 | Kept → V11 | Names, tooltip and focus retained. |
| UI08 | Kept → V12 | Complete keyboard path retained. |
| UI09 | Kept → V13 | Ordered, visible focus retained. |
| UI10 | Kept → V15 | Consequence review and applied-state distinction. |
| UI11 | Merged → V16 | D4/D9 also remove duplicate cards/gaps. |
| UI12 | Kept → V17 | Empty groups and attention order; direct empty page clarified. |
| UI13 | Merged → V18 | D1/D6 add the complete flowing-row constraint. |
| UI14 | Kept → V19 | Same counts and unknowns; icon exception explicit. |
| UI15 | Kept → V20 | Exact existing geometry. |
| UI16 | Kept → V21 | Same alignment and count growth. |
| UI17 | Kept → V22 | Same base/secondary values and independent icon sizing. |
| UI18 | Kept → V23 | Same resize/reflow protection. |
| UI19 | Kept → V24 | Stronger local contrast retained. |
| UI20 | Kept → V25 | Same graphics contrast and exceptions. |
| UI21 | Kept → V26 | Natural fit, one scroller, reachable last action. |
| UI22 | Kept → V27 | Added loading/no-match fixtures, no state conflation. |
| UI23 | Kept → V28 | Full effect outcomes and retry protection. |
| UI24 | Kept → V29 | Non-disruptive announcements retained. |
| UI25 | Kept → V30 | All disclosure, summary, keyboard and no-hint clauses retained. |
| UI26 | Kept → V31 | All three aggregate states. |
| UI27 | Kept → V32 | Exact set operations and empty-selection default. |
| UI28 | Kept → V33 | View-only, count, restoration, Reset and old shortcuts. |
| D1 | Merged → V18 | One entity block. |
| D2 | Merged → V03/V21 | One header/grid and visible-edge alignment. |
| D3 | Merged → V22/V23 | Readable adjustable type and scaling. |
| D4 | Merged → V16/V20 | Purposeful space and no duplicated context. |
| D5 | Merged → V08/V34 | Direct actions/links, optional detail. |
| D6 | Merged → V18 | Full-width wrapping and intact identifiers. |
| D7 | Merged → V24/V25 | Stable non-color semantics and contrast. |
| D8 | Merged → V08/V11–V14/V27 | Real destinations, names/focus and explained unavailability. |
| D9 | Merged → V16/V18/V21 | Density without smaller type or count collisions. |
| D10 | Merged → V19/V21 | Stable semantic count groups and unknowns. |
| D11 | Merged → V09/V11/V34 | Inline actions without losing targets/names. |
| D12 | Kept → V37 | Concise canonical documentation. |
| D13 | Merged → V17 | Decisions/faults before routine/finished work. |
| D14 | Merged → V34 | Section actions and settings grouped by recognizable purpose. |

## Evidence and adoption boundary

Baseline: `3a2300a2`, approved #1348 checklist and pinned shared revision
`d496acf9365edfe1c11132f363ae5aaa7573e1f6`. Route inventory checked against
`src/tray-app.js` and [M8/WS11](menu.md#m8-implementation-boundary);
workspace targets against [AI workspace](ai-workspace.md).
Existing [#1348 failing observations](ui-rules.md#observed-menu-findings) remain
unchanged. No UI fix is claimed; there is no runtime after-pass in this proposal.

Document review checks: 38 unique V IDs; all 28 UI and 14 D IDs mapped; sources
and agreement/conflict recorded per rule; route/header/modal/link/return cases
specified. Implementation, accessibility and native acceptance are **Not tested**.
A future implementation must retain before-failure and after-pass evidence for
its changed paths, not accept this document as a pass (R12, § 7).

Approval would amend the TaskQ R13/Product design-target pointer and linked
checklist in the same reviewed delivery, preserving old identifiers and baseline
evidence. Until then this file is a proposal only; no active rule is amended.
For shared adoption, propose the generic V checks in specq design/, replace
this generic prose with a versioned link, and keep routes, values, filter semantics,
fixtures and exceptions here. Review each consumer’s compatibility explicitly;
TaskQ approval does not prove adoption by DashQ or any deployed runtime.

## Owner decision (A5)

1. Approve v2 and move its generic checks to specq design/ through a reviewed
   upstream change; retain TaskQ routes/values/filters here. Recommended: one
   shared rule home, with explicit consumer compatibility review.
2. Approve v2 for TaskQ only; postpone shared adoption. Smaller publication
   scope, but other products continue using existing D rules.
3. Revise v2 before approval; name the V IDs and desired behavior. Existing
   UI01–UI28 and D1–D14 remain active while the proposal changes.

Answer through the #1442 ask card. No dependent implementation starts before
that answer; shared publication and consumer adoption need their own evidence.
