# Completed reload regression (#1711)

Completed preserves each selected source's displayed rows until its first successful replacement page. A failed or missing response retains them with an explicit previous-rows label and source-specific Retry. Search, outcomes, pins and paging keep their existing behavior after replacement.

Applied rules: R13 Product and MP14 define the behavior before implementation; R12 requires failure/pass evidence and keeps browser proof separate from native acceptance; § 7 requires source pins and the selected candidate gate. No R-number changes.

## Failure before, pass after

- Baseline: `66641866f8cf6844175e8fd7aa70b40f5a692aa1`.
- Source candidate: `7df61837b4356801f1254208a19c7541c56a9a3a`.
- Command: `node tests/task-dashboard.browser.cjs`, using `npm run build` artifacts and invented board replies; no live board mutation.
- Before: the added A1 reproduction expected five displayed rows after period/outcome changes, but received `[]` (`A1: slow filter reload keeps the previous rows`).
- After: the same reproduction passes. A1–A4 also cover independent source errors, a missing response at 65 seconds, source-only Retry, Reset + Done, first-page replacement, duplicate/late reply rejection, and failed-page retry without losing earlier pages.
- Retained protection: existing search/outcome/merge-receipt distinctions, pins, pagination, view persistence, keyboard controls and dark/light 16/20 px geometry cases remain in the same browser check.

## Render review

Environment: macOS host, Node v22.18.0, Playwright 1.64.1, Chromium headless 156.0.8078.4; actual bundled tray UI, dark theme, 16 px, 360 × 760 viewport. Visual QA inspected both images at natural size. Required rows, wrapped source text and Retry controls are visible without horizontal clipping; the list and notices use the existing content scroll. No icon, palette or row-geometry change from the pinned visual reference is intended.

| Acceptance | Evidence | Result |
|---|---|---|
| A1: slow/failing reload retains rows | Same five-row assertion before/after; [errors](completed-reload-errors.png) | Pass, browser |
| A2: per-source progress/error/Retry | Independently delivered error and simulated missing reply; retry targets only Alpha | Pass, browser |
| A3: Reset + Done restores full criteria | [Reset loading](completed-reset-loading.png); replacement includes a success after Not planned filtering | Pass, browser |
| A4: accessible text and roles | Reading/loaded `status`, failure/timeout `alert`, named Retry buttons | Pass, DOM/browser |

## Limits

The 65-second deadline bounds the UI's wait for a reply, including native queue time; it does not cancel the existing read-only native command. Native commands retain their existing timeout and queue. Late replies are ignored after timeout or replacement.

Installed native Completed behavior, Windows and screen-reader announcements were not exercised. These browser checks do not claim live/native acceptance. The README snapshot refresh uses an isolated installation and does not change the running service.
