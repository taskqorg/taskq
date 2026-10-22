# Dashboard rebase acceptance (#1578)

This candidate retains the approved dashboard and resolves publication conflicts with main `ab57340d2088f0bdf22c52f675ae1bc19e87b993`. R6/R8/R12/R13 and §7 apply. The [original F5 ledger](../report.md) and frozen reference remain unchanged; this report supplements their revision-specific evidence.

## Failure and resolution

- Before: PR #1590 reported `mergeable: CONFLICTING`; `git rebase origin/main` stopped in `src/tray-app.js` and `docs/shots/manifest.json`.
- Resolution: retain #1587's physical history raster, exact counts/ranges, selected-bar handler, resize observation and chart CSS together with dashboard placement. Keep the accepted identity-anchored task actions; do not restore the old right ellipsis. Retain #1589's native-history mtime fix unchanged.
- The source rebase completed. `git range-diff` confirms the six original commits retain their changes; only the render/click integration and generated screenshot provenance needed reconciliation.
- Combined browser checks found a real accessibility gap: the graph's enabled bar hit rectangle was 4×64 CSS px in the initial narrow fixture, below the 32/44 px target. Add a full-sized native interval selector as the equivalent target permitted by UI06; every exact bar remains directly keyboard/click accessible and retains R-UI5 geometry. This implements the existing exception, without changing a product decision.
- After: `tests/shell-navigation.browser.cjs` passes fine/coarse target dimensions, accessible names, focus and overlap across all routes. Its bar exception applies only when an enabled, visible selector meets the full target dimensions and offers every bar. `tests/tray-history.browser.cjs` retains all raster/count/range checks and proves that both entry points select the same exact interval.
- Test change: the defect is the missing full-sized equivalent, not the quantitative raster. Both target access and exact histogram protection remain. No failed assertion was removed without replacement protection.

- Fresh WKWebView capture exposed a second boundary: the system select painted at about 18 logical px high despite the minimum CSS target. [Failing native capture](native-before-select/popover-dark-cadence-16.png) is preserved. Apply the existing control styling with `appearance:none`, inherited font/target dimensions and the shared Lucide arrow; the selector still uses the native selection API. Replacement native captures under `native/` show the full target, inherited 16/20 px text and shared arrow in both themes, with no horizontal overflow.

## Evidence boundaries

Fresh browser captures and measurements are under [browser](browser/browser.json), [shell](shell/routes.json) and [history](history/after-real-menu-2x.png). These use invented/token-free data and the actual production bundle. The inherited native and reference packages remain historical evidence of their recorded revisions, not fresh proof of this binary.

A plain native `<select>` in macOS headless Chromium did not perform ArrowDown selection, independently of the app. The interval check therefore uses Playwright's native selection/input boundary, tests focus, and retains real Enter activation of graph buttons. Native popup keyboard interaction remains unqualified; no custom keyboard emulation or retry gate was added.

Native captures, final artifact hashes, check results and source identity are recorded in [candidate.json](candidate.json). Visually inspected examples and limits are listed there. Native click/raise remains owner-observed under U-A8; Windows/DPI, physical drag/Snap, screen reader/IME and live operational effects remain unverified. No deployment, service restart, grants, models or admission changes were made. Merge-result `merge_group` CI remains the publication gate (§7/R12).

## Final checks

- Source is frozen at `2655b78d823a576552168f410fca454e6982c4f1`. The pinned main is its ancestor; no rule was amended by this rebase repair.
- Full selected gate: 481 passed, 18 worker-authority cases sandbox-blocked, 34 ignored in 39.240s. Integration: 19 passed, 1 `sandbox-exec` case sandbox-blocked, 1 ignored. These are not PASS; no worker guard or permission was bypassed. Rust code stayed unchanged after that gate; subsequent selector changes were checked through the final production bundle/native artifact.
- Core model: 9 tests passed, including 97 golden traces and table coverage. Contract remains 298 lines and 61436 bytes (limits 300 lines/61440 bytes).
- Final source: all 21 Node component instruments and screenshot/release-script fixtures passed. All 12 browser instruments passed, including 64 shared comparisons, O1–O3, both themes, 16/20 px, long/short/partial data, keyboard/focus, target access and history raster/counts.
- Eight fresh signed WKWebView captures (`home,cadence`) passed with no horizontal overflow. The same signed artifact passed offline selftest: 97 golden traces, verdict formats and 9 committed telemetry observations; board read remains unknown/offline.
- README images were regenerated with `docs/shots/update.sh`; `docs/shots/check.sh` and whitespace checks pass. No live service installation, board operation or native gesture is inferred from snapshots.
