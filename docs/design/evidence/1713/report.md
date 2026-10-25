# Settings navigation — #1713

R13 Product and TW1–TW2 keep Settings inner pages in Settings and keep explicit
operation reviews in the workspace. This change covers navigation and route titles.
It leaves operation review content, authority and native operation handling intact.

## Reproduction and acceptance

Base: `66641866f8cf6844175e8fd7aa70b40f5a692aa1`.
The extended `node tests/settings-content.browser.cjs` failed before the source fix:
`#1713: inner title equals list label`, actual `Settings`, expected `Work mode`.
The same instrument passes after the fix (R12, § 7 step 4).

| Acceptance | Observation | Result |
|---|---|---|
| A1 | Work mode opens an inner Settings page; Back returns to the list and restores its link's focus. | Pass, browser |
| A2 | Recovery opens an inner Settings page; Back returns to the list and restores its link's focus. | Pass, browser |
| A3 | Agents and access and Services and support use their full list labels as titles. About and updates matches too. | Pass, browser |
| A4 | Opening and refreshing the inner pages sends no operation or workspace handoff. Explicit review links retain their original operation/target. | Pass, browser |

The existing bundled Settings instrument retains widget, sign-in, unknown/access,
host service and Appearance protection. Its added cases run at 320 px in both
themes, with and without host capacity. An absent capacity projection retains the
admission review destination; a present one retains the resources review.

After images show the actual bundled Settings surface with invented data, 16 px,
320×900, dark theme, Windows headless Chromium. They are browser evidence, not
native window or live operation proof:

- [Work mode](settings-work-320-dark.png)
- [Recovery](settings-recovery-320-dark.png)

Before native evidence: [#1703 report](../../review-evidence/native-1703-2026-10-25/report.md),
screens 04, 06, 08 and 09.

## Additional checks and limits

`node tests/window-chrome.browser.cjs` passes: Settings chrome, Home/Back and
independent menu geometry/navigation remain protected. The existing
`node tests/tray-recovery.browser.cjs` passes its selection, evidence, unknown and
same-payload reconciliation checks. Neither instrument performs native effects.

Two broader optional instruments fail on both this change and the base source:

- `node tests/shell-navigation.browser.cjs`: `G02/V23: essential Back/Close clipped at 320px / 200% text`.
- `node tests/workspace-operations.browser.cjs`: `Cannot read properties of undefined (reading 'all')`; its snapshot lacks the required display model.

The base comparison bundles the exact base `src/tray-app.js` with unchanged
dependencies and remaining source, then runs the same instruments and fixtures.
These pre-existing failures are not claimed as passes. No shell layout or obsolete
fixture repair is included in this Settings navigation task.

Native Settings clicks/window raising, macOS rendering and live operation effects
are not tested. Publication requires green merge-result CI (§ 7, R12); worker
handoff is not merge, release or deployment acceptance.
