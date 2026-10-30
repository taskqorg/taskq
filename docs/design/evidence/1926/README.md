# Menu and Settings polish — #1926

Implement UX30-01..05 in the production components. The owner selected option 1 on #1926: amend Product, M2, M3 and M1.2 for the neutral project rail and opaque sticky counter row. Collapse, shown/total counts, direct destinations and whole-window opacity remain unchanged (R13).

## Candidate and boundary

Before: `be3c20f3337d8db3e2c7bb555bbae777247c2bb2`. Production UI changes: `331cfe90cd6da17b6421d8f1cd4a9e2470e07495`; the final task branch also clears counter scroll-padding when leaving Home, keeps the contract within R8, and adds this evidence and a visible-text test correction. Rebase onto origin/main completed before the first push. Screens use the built `tray.html`, with intercepted IPC and invented observations from `tests/ui-polish.browser.cjs`. No projection adapter, live task operation, service change, native grant or saved user view was used.

Reference: the verified Oct 30 ZIP (`e397147ee8fe6d6867af8ecfe604141e9921b0ce6c46e2f059a03851e9b3342d`), its README, plan, acceptance, issue map and rendered live-polish index. Apply its scoped deltas; keep current main behavior. Deliberate difference: both opacity regulators and their disabled states stay present, as A5/A6 require. No inactive saved value is hidden or removed.

Browser evidence was collected on macOS with Playwright Chromium headless, 360×560 CSS px, dark, 16px, 1× for the representative captures. The matrix covers both platform projections, both themes, 14/16/18/20px, 320/360px and 13 routes (Home plus all 12 Settings routes). Windows projection here is still Chromium on macOS; it does not prove Segoe, WebView2, native DPI or installed Windows rendering (R12).

## Failure before / pass after

The same instrument fails on the before bundle: counters disappear at middle/bottom, the strip has no opaque fill, state slots do not share a centered target-sized column, and project headings lack the specified rhythm. The [before observations](before/checks.json) retain those seven failed checks. The after bundle passes them, the real wheel/keyboard checks and the [416 geometry cells](after/checks.json). The [source control inventory](before/controls.json) compares equal on all 12 routes, including hidden and disabled controls, labels, tooltips, values, exact destinations and operation data.

Ten selected screens, one before/after pair per distinct observation:

| Observation | Before | After |
|---|---|---|
| Shared status slots and neutral project rail | [Top](before/menu-top.png) | [Top](after/menu-top.png) |
| Sticky row at mid-scroll, opaque fill and hairline | [Middle](before/menu-middle.png) | [Middle](after/menu-middle.png) |
| Sticky row through the end; Reports retained | [Bottom](before/menu-bottom.png) | [Bottom](after/menu-bottom.png) |
| Appearance icon headings; opacity value retained | [Appearance](before/appearance.png) | [Appearance](after/appearance.png) |
| Separate client/access groups and aligned disclosures | [Agents](before/agents.png) | [Agents](after/agents.png) |

Inspected all ten at natural size against the reference. The matrix checks container and actionable-section scroll widths; root clipping is not counted as fit. No added scrolling pane. Wheel gestures over the pinned controls continue the existing content scroll; keyboard focus reaches the last task below the strip. Background opacity 0 still leaves the strip opaque; whole-window opacity keeps its existing native behavior.

## Acceptance and retained protection

| Item | Source/browser result | Limit |
|---|---|---|
| A1 / UX30-01 | Pass: target-sized centered slots, common identity edge, pinned asset/palette reuse, four text sizes and coarse-pointer 44px slots | Native scale/type calibration not tested |
| A2 / UX30-02 | Pass: shared max(16px, 1em)/4px heading rhythm in project bands, project overview, Settings and conversation groups; dense task rows retained | Native engines not tested |
| A3 / UX30-03 | Pass: neutral rail; existing project-group test retains exact board/root IPC, unavailable folder, overview/Back/collapse/query/scroll | Real OS folder open/denial not exercised |
| A4 / UX30-04 | Pass: existing view/search/Held handlers unchanged; dashboard suite retains active/no-match search, exact scope, counts, revisions and view persistence | No real admission during qualification |
| A5 / UX30-05 | Pass: 12-route control parity and geometry; existing Settings suite retains sign-in/access/versions/units/freshness and explicit operation review | Native settings persistence not exercised |
| A6 | Pass: unchanged views, filters, global pins, Completed, notifications, Reports and action/disabled identities; existing dashboard and Settings regressions retained | Live effects not exercised |
| A7 and sticky note | Pass: before/after evidence, 416 cells, real wheel and keyboard checks in whole/background modes; project matrix adds 48 cells and touch targets | Native macOS/Windows integrated qualification belongs to #1932/#1933; merge-result CI pending |

A targeted route probe also reproduced leaked counter scroll-padding in Settings (`40px` at a wide viewport). The final controller clears it on departure and restores it on Back; the UI polish chain passes both transitions.

The first canonical gate failed after 150.624s: the new Product wording exceeded R8 (61,510 bytes). Only that amended line was shortened; the contract is now 61,438 bytes against 61,440. `cargo test --locked --bin taskq core::tests::every_table_row_has_a_golden_trace` passes with its size/trace assertions unchanged. The full gate is repeated because the candidate and the failed evidence changed.

The Settings host-label test now reads `innerText` rather than SVG indentation in `textContent`. Expected host names, exact runtime counts and stale/expired facts are unchanged. The new chain protects sticky scrolling and alignment, rather than asserting source strings. The sidebar check adds the accepted rhythm while keeping its existing grouping, search, archive, focus and draft guards.

## Checks

- `node tests/ui-polish.browser.cjs` with the before inventory: Pass; 416 cells, 12-route parity, status slots, sticky top/middle/bottom, wheel and keyboard; whole/background modes.
- `node tests/menu-project-groups.browser.cjs`: Pass; 48 platform/theme/font/width cells, 32/44px targets, exact targets and retained context.
- `node tests/task-dashboard.browser.cjs`: Pass; scoped counts, comparator, views, Saved/Reset, pins, Completed, task controls and short/narrow layouts.
- `node tests/settings-content.browser.cjs`: Pass; original facts and inner-route review boundaries in both themes at 320px, with/without capacity.
- `node tests/conversations-sidebar.browser.cjs`: Pass; shared rhythm, original grouping/recency, archive, focus/drafts and no execution effects.
- `node tests/tray-settings.test.cjs`, `node tests/tray-navigation.test.cjs`, `node tests/tray-running.test.cjs`: Pass.
- Canonical candidate gate: pending recording below.

No deployment or live native acceptance is claimed. Revert the source commit to undo the presentation change; no user-data migration or new dependency is involved. Native/OS/provider checks and green merge_group CI remain separate gates under R12 and §7.8.
