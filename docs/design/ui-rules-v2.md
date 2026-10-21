# App UI rules v2 — TaskQ profile

The [owner answer](https://github.com/taskqorg/taskq/issues/1442#issuecomment-6310011259)
approves v2 and moving generic checks to specq through review (R13, § 7).
Apply the **[38 shared V checks][V]** plus the TaskQ parameters below, extending
[UI01–UI28](ui-rules.md#checklist-28-rules) and [D1–D14][D]. Shared sources and
per-check agreement/conflict live upstream; this file does not duplicate them.
[Upstream PR #73](https://github.com/taskqorg/specq/pull/73) is submitted for
review, not yet claimed merged. The link pins its exact candidate revision;
publication review must verify that upstream revision is accepted before claiming
shared adoption. No deployed consumer adoption is established by either PR.

These are approved design targets for tray/workspace, not shipped behavior,
WCAG conformance or new runtime authority (R8/R9/R12). Other consumers keep their
own routes, dimensions and filters; no DashQ implementation is changed here.

## TaskQ navigation model

This is an approved logical route tree, not a claim that URL handlers exist.
It preserves current `src/tray-app.js` routes and the accepted workspace entry.
Logical identifiers below map to URLs only through a consumer’s route ledger.

```text
Home (root)
├─ Attention
├─ Projects
│  └─ Project {board, project-id}
│     └─ Tasks {selected states} (counter strip)
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
├─ Messages (root) → Archived
├─ Task answer {board, task, revision}
├─ AI request {project, runtime}
├─ Operation review {target}
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

[TW1](tray-workspace.md#contract-detail) places these slots in one fixed bar per screen, with breadcrumbs on nested project/settings routes; dialogue and reviews live only in the workspace ([TW2](tray-workspace.md#contract-detail)). Header slots: leading Back (reserved at root), one route title next, trailing
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
These are approved acceptance checks, not evidence of current persistence.

## TaskQ parameters for the 38 shared checks

This is a parameter/exception table, not a second generic checklist. For a V ID
not listed here, use its shared check with the fixture boundary defined by
[TaskQ UI acceptance](ui-acceptance.md). CSS px, Apple pt, Android dp and physical
pixels are different units; native scaling needs separate qualification.

| V IDs | Local acceptance parameter or explicit interpretation |
|---|---|
| V01–V07 | Use the navigation model above, including header slots, modal scope, direct-entry errors, URL privacy and return/refresh restoration. V02 position tolerance is ≤1 CSS px while scrolling. V03 fixtures: 320/360 px with 14/16/18/20 px type. |
| V08 | Board beside project identity; task body opens issue; session and PR links remain independent per M3/D-A2. |
| V09/V10 | Fine-pointer hit boxes ≥32×32 CSS px; coarse-pointer ≥44×44, actual nonoverlapping bounds. Conflict with Android 48 dp is deliberate, not unit equivalence. WCAG exceptions do not waive this product target. |
| V11–V15 | Apply UI07–UI10, including tooltip/name/focus, standard Enter/Space behavior and target/effect review. Emergency stop and window Close remain separate; saved/pending/applied are distinct. |
| V16/V17 | No repeated identity, destination, counter group or decorative card without distinct context. Empty summary groups occupy zero rows; decisions/faults precede ordinary lists and completed/maintenance content. Direct empty Attention still explains its state. |
| V18 | At 320/360 px with 180-character titles, status/issue/title start one flow and continuation uses the full block width. Keep atomic issue numbers and host names intact. |
| V19/V21 | Counts 0/1/12/100/1000 and partial data: stable semantic order/right-aligned groups, labelled unknown, no menu capsule for zero; summary-icon zero slots remain the fixed-position exception. No count collisions or unrelated stretched gaps. |
| V20 | Tray outer inset 8 px, row horizontal inset 4 px, related gaps 4–6 px, section 6 px with one 1 px divider. Other surfaces declare their own tokens. Do not force an 8-only grid. |
| V22 | System font; base b = 14/16/18/20 CSS px, default 16; body/action b, secondary ≥max(12 px, 0.85b), section heading b semibold, route title 1.1b semibold. Header/section roles are approved v2 additions; other values come from UI17. Font persists throughout; summary-icon calibration stays independent and in-content symbols remain legible. |
| V23–V26 | Retain UI18–UI21: text contrast ≥4.5:1 even where WCAG allows 3:1 for large text; essential graphics ≥3:1 with documented exceptions. Keep 200% text resize, 320 CSS px ordinary-text reflow, natural native content-fit and one intended scroller with reachable final control. |
| V27–V29 | Retain UI22–UI24: missing/stale/failed/empty never become zero or healthy; loading/no-match/partial remain distinguishable. Effects use pending/readback success/failure/uncertainty with safe retry; announce non-focus updates without moving focus. R12/M5 own truth and receipts. |
| V30 | The task-list counter strip is the state filter (M3, [TW8](tray-workspace.md#contract-detail)): capsules toggle with Enter/Space and expose `aria-pressed` with the selected outline; the strip shows selected states or All tasks. No Filter checkbox group, no usage hint. |
| V31 | All tasks is pressed exactly when no state/project restriction applies; visual and accessible state agree. |
| V32 | Selected states use OR; selected projects AND selected states. Zero selected states means no state restriction; Reset restores all. |
| V33 | Filters are view-only: selection/match count visible, Back preserves it, zero matches offers Reset, old single-state counter shortcuts stay. No queue/focus/capacity/label write. |
| V34–V38 | Apply existing D review and inline-action rules. Reduced motion removes nonessential spatial animation; no expressive bounce default. Keep essential controls stable and inspect normal/blocked/critical/frequent-use fixtures. The composed checklist stays 38 checks (target ≤40), not an extra set of local rules. |

The compact title and role scale intentionally differ from Apple iOS large-title
preferences and Material/Fluent default type ramps. Native iOS consumers use
scalable semantic styles and qualify Dynamic Type; macOS does not support Dynamic
Type. Native chrome stays OS-owned. Product-specific target and spacing choices
preserve the accepted dense operator profile, not a universal platform mandate.

## Preservation map

Kept = same protection; merged = retained within a combined check. None deleted
or weakened. This table is traceability, not another checklist or adoption of D
outside its canonical home. UI01 is clarified for workspace roots/deep entry,
not waived on any existing tray child route. The owner approved the added detail in #1442.

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
| UI25 | Merged → V30 | Summary, keyboard and no-hint clauses retained on the counter strip; disclosure replaced by M3. |
| UI26 | Merged → V31 | Aggregate state carried by the All tasks capsule. |
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

Baseline: `3a2300a2`, approved #1348 checklist and original shared revision
`d496acf9365edfe1c11132f363ae5aaa7573e1f6`. Route inventory was checked against
`src/tray-app.js`, [M8/WS11](menu.md#m8-implementation-boundary) and
[AI workspace](ai-workspace.md). The owner’s #1442 answer follows the proposal
and ask; this approved profile does not substitute an agent decision for that answer.

The [#1348 failing observations](ui-rules.md#observed-menu-findings) remain
unchanged. No UI fix or runtime after-pass is claimed. Implementation,
accessibility and native acceptance are **Not tested**. A later implementation
retains before-failure/after-pass evidence for changed paths (R12, § 7).

Compatibility: all UI01–UI28 and D1–D14 protections survive in the map above;
D remains canonical upstream. This profile amends the R13/Product target pointer,
not R6 task-row/runtime semantics or R9 authority. Other consumers must map their
own entities, values and old acceptance IDs and verify representative output
before claiming adoption. A shared merge alone proves neither integration nor
runtime behavior. Source provenance and vendor differences are recorded in the
pinned [shared checklist][V], not copied into this product profile.

[V]: https://github.com/taskqorg/specq/blob/d54981e576a80c3854b455c5eb1a441a676f4497/design/app-ui.md
[D]: https://github.com/taskqorg/specq/blob/d54981e576a80c3854b455c5eb1a441a676f4497/design/operator-ui.md
