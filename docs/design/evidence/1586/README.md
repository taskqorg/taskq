# History bars: #1586

Rules applied: taskq.md R12/R13 and §7; rendering contract R-UI5, icon I4/I6 and menu M6. R-UI5 was amended before implementation. No queue, data collection, period alignment or other menu layout changes.

## Before

Base: `21742cea80e5ac90c0e9e375169fecaae8e09773`.

[Installed menu, real aggregate data](before-real-day.png): Day, closed 58, failed 0, ask 30, waiting 19, peak 18. The menu bars were already wide at this window size. Reading was difficult because it lacked a color legend, duration per bar and complete time ranges/counts for an individually selected bar. Unknown older history also required reading the separate warning. This image records the actual installed macOS WKWebView, not an invented fixture.

The new Rust raster regression failed on the unchanged renderer: `16px @1x: [1, 1, 1]`; 0 passed, 1 failed. The new menu browser check failed because the bin-count/duration label did not exist.

[Before, TaskQ-only event replay](before-project-replay.png) uses [captured histogram](real-history.json): closed 42, failed 0, ask 26, waiting 15. Only histogram counts and bounds are saved; no tasks or event identities. The installed screenshot covers all registered projects, so its totals differ. Compare this replay with the after images for identical data.

## After

- [Menu at 1x](after-real-menu-1x.png) and [at 2x](after-real-menu-2x.png): the same TaskQ-only histogram, exact totals, start/end, peak 17, 12 bars at 2 h each, legend and selected last-bar details. Chromium bundled-product instrument, not an installed deployment.
- [Rust icon rasters](icons.png): 16 px at 1x, 22 pt at 1x and 22 pt at 2x, native raster plus nearest-neighbor 4x inspection. The icon intentionally contains no counters in this fixture, isolating the graph. All four series remain; narrow slots sum more source bins. Native OS slot scaling is not qualified by PNGs.

Environment: macOS 27.0.1 arm64, Node 22.18.0 / Playwright 1.64.1; browser system sans-serif and Rust system-font path. Geometry uses physical pixels, with one-pixel gaps and a four-pixel minimum bar. The menu retains a 64-logical-pixel plot and one shared linear scale. Clicking or keyboard-activating a bar exposes its exact range/counts below the plot; titles provide hover detail.

## Verification

- `cargo test --locked history_ -- --nocapture`: 8 passed, including exact adjacent sums, every series total, one-bar widths, native icon widths 16/20/22/24/32/44/64 and existing histogram/time-boundary checks. The two new tests are persistent protection against narrow bars or dropped/misgrouped series.
- `node tests/tray-history.test.cjs`: passed; divisor thresholds through one-bar/no-fit cases and real icon/menu widths preserve all four series.
- `node tests/tray-history.browser.cjs`: passed at 1x/2x for 5 min/hour/day/7 days/30 days and one custom range. All periods retain identical geometry, order and scale algorithm; only duration changes. Checks cover cumulative physical segment heights, bin details via keyboard, unknown labels, large light text and resize. A real pointer click selects the final bar after resize. The shared button hit-target minimum initially widened bars beyond their allocated plot; the regression now requires exact computed widths and an in-bounds last bar.
- `python3 scripts/check.py --base 21742cea80e5ac90c0e9e375169fecaae8e09773`: sandbox-blocked after 103.549 s. Rust reported 479 passed, 18 failed, 34 ignored. Host-policy/install/Stop fixtures encountered worker-only refusals, including `a worker cannot change host policy` and `a worker cannot restart the host service or request Stop`; related receipt assertions then failed. Worker authority was retained; this is not a passing full suite. The gate stopped before integration and Node checks.
- All `tests/*.test.cjs`, `docs/shots/check.test.sh`, `.github/release-tag.test.sh` and `.github/release-check.test.py` passed separately.
- `cargo run --locked -- selftest` with an isolated installation and `TASKQ_SELFTEST_OFFLINE=1`: passed 97 golden traces, formats and staged capture. The board was not read.
- `docs/shots/update.sh` and `sh docs/shots/check.sh`: passed; README shots/manifest pin source `0b77bbb0de8fbfb85d4798e5d972aba754fed556` and binary SHA-256 `77d459efd7c7f6e68753cec7e0f0b5d0ae1bbb3db6b307db3a18d5acfe1469dd`.
- `cargo run --release --locked -- tray --snapshot <isolated-dir> --route cadence`: passed in isolated `docs/shots/fixture.sh` data, both themes at 16/20 px. [Native WKWebView example](native-cadence.png). This is a signed source build, not a live installation.
- The images were visually inspected at natural size and enlarged. Windows native placement/scaling and owner judgement of final readability remain unverified. Publication still requires the reviewer and green merge-result CI under §7; no live installation was performed.
