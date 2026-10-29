# Exact Summary counters · #1789

R13 amends Product and [I1/I2/I5](../../icon-summary.md); M1 links the revised fitting rule. R12 and §7 distinguish renderer checks from installed/native acceptance. The baseline is `6a97fc111c428213c136f2b54a35f9214586334c`.

The fixed 5.6 pt cells replaced two digits with `+`. Both native-font implementations now return measured ink. The shared layout reclaims hidden-zero cells, then expands the Mac menu-bar canvas only when needed. Dock and Windows retain square canvases containing the complete SVG. The chart, palette, unknown dash, zero hiding, compact fallback and pause policy remain protected.

## Before and after

A temporary Cargo example compiled the baseline `src/icon.rs` and candidate renderer against the same locked dependencies, without the service or board. At 9 pt SF Medium, the first counter for 10 or 12 was a 4.761406 pt plus. Afterward, 10 measures 9.19878 pt and 12 measures 8.890751 pt. A lone 12 keeps the 22 pt slot; 12/3/15 needs 27.613152 pt. Three capped counters use 55.484924 pt. These are logical renderer dimensions, not observed installed slot sizes.

The initial development test incorrectly treated the baseline as the bottom ink bound; SF numeral overshoot made its 9.01 pt assertion fail on a single digit. The corrected bound is the chart's first possible mark at y=10.5. The independent baseline/candidate instrument then confirmed the actual overflow defect above. No chart, font or overlap protection was weakened.

The three inline panels retain the light-theme before/after pairs: six views total, with original 2× rasters, nearest-neighbor detail and square Dock exports. Dark-theme matrix exports were inspected but are not retained as duplicate review variants. All activity is synthetic. These are **canonical renderer exports, not installed menu-bar or Dock screenshots**. The native SF outlines are real; OS placement, translucency and presentation are not qualified.

![8: preserved single digit](8.png)

![12: exact two digits replace the plus](12.png)

![12/3/15: separate counters use a wider composition](mixed.png)

Visually inspected all three panels, both themes, and the 256 px export with three `99+` counters. All digits and six columns are present; no counter overlaps another or the chart. Unaffected compact assets and the accepted six-column chart remain the reference.

## Acceptance

| Item | Result | Evidence |
|---|---|---|
| A1 specification and renderer | Pass | I2 changed first; exact 10–99, 99+ cap, measured cells and separate Mac/square raster paths |
| A2 counter matrix | Pass | `icon::tests::exact_two_digit_counters_reclaim_space_before_widening`: 0/9/10/12/99/100, each position alone and together, both themes, 22/44/256 px |
| A2 retained protection | Pass | Existing icon tests retain palette, six bins, unknown/zero, blocked aggregation, pause, compact resources and first-capture behavior |
| A3 installed Mac 8/12/12/3/15 | Pass (menu bar, dark) | Native status-item captures from an isolated candidate tray: [native/README.md](native/README.md). Dock and light appearance not tested |
| A3 Windows availability | Not tested natively | Square notification-area canvas is retained; target-specific execution and merge-result CI remain pending |

The counter matrix replaces the old oversized-count width ceiling, whose accepted behavior changed. Single-digit/unknown ink alignment remains checked. The new cap case compares 100 with the largest usize. The adapter check `tray::tests::two_digit_summary_uses_the_native_canvas` protects the Mac rectangle and Windows square selection and shared published/native SVG path; it does not create an OS status item.

Environment: macOS 27.0.1, arm64, SF system font Medium 9 pt; rustc/cargo 1.98.1, warm seeded dependencies. Source hashes and panel hashes are pinned in `manifest.json`. The source check and canonical gate receipts follow below.

- `cargo test --locked --bin taskq icon::tests`: 6 passed, 1 subprocess helper ignored; 19.23 s execution after the warm build.
- `cargo test --locked --bin taskq two_digit`: 2 passed, including cap and native-canvas checks; 3.14 s execution, 60 s warm rebuild.
- The temporary baseline/candidate export instrument confirmed the old single-plus signature and the corrected two-glyph ink for 10 in both themes; 8 stayed identical.

The expanded pause mask retains the original cleared Q region and also covers the extended canvas. A pixel check requires counter ink beyond the former 22 pt edge while paused, preventing a new clipping regression.

- `RUSTUP_TOOLCHAIN=stable sh docs/shots/update.sh`: passed from `de0f48ea`; the source-pinned README snapshot and icon export were opened and inspected. The project PNG is unchanged; the clock-dependent icon export was refreshed.
- `sh docs/shots/check.sh`: passed. This verifies source pins and capture files, not installed OS icon presentation.
- The first release build refused the inline dynamic-mask `write!` with `instrumentation changed source lines`. The equivalent multiline `push_str(format!(...))` form passed instrumentation and the release build; the mask geometry is unchanged.

The first canonical gate on `4f77e899` ran 233.243 s: 564 passed, 19 failed, 33 ignored. All icon checks, including the expanded pause mask, and the native-canvas adapter check passed. One failure was ours: Product prose grew the 61,438-byte contract beyond its 60 KiB R8 limit. Product now links exact counts compactly; the six-bin decision remains in I3. The 18 other failures exercise host install, capacity, recovery or Stop under worker authority. No permission guard or test assertion was relaxed. The corrected contract is exactly 61,440 bytes and 300 lines.

## Corrected candidate receipt

On `3a33a1a5`, `python3 scripts/check.py --base origin/main` is **sandbox-blocked, not PASS**: 565 passed, 18 host-authority cases failed, 33 ignored; 155.279 s / 900 s. The R8 contract-size/golden-table check and all icon/canvas tests passed. Explicit refusals include `a worker cannot install the host service`, `a worker cannot change host policy` and worker Stop/restart refusal. Integration tests were not reached. Worker authority and every assertion remain unchanged.

The gate stopped before its later stages. Those were run separately: all 25 `tests/*.test.cjs` component scripts and three artifact scripts passed (28 checks, 55.493 s; exact commands in the manifest). Isolated `TASKQ_INSTALL`, `TASKQ_SELFTEST_OFFLINE=1` with `cargo +stable run --locked -- selftest` passed 99 golden traces and formats, committed 9 staged observations and intentionally left the board unknown. The running collector was not used.

Final evidence edits only reduce the review set to three light before/after pairs and record receipts/hashes. The tested Rust and contract bytes are unchanged. `8` was also compared byte-for-byte before/after in both themes and is identical. The three final sheets and both README captures were opened and inspected. Native Mac menu-bar evidence was subsequently supplied by #1792; see the separate [native receipt](native/README.md). Dock presentation and Windows native availability remain Not tested; publication needs green merge-result `merge_group` CI (R12, §7).

## Resumed candidate

Resumed after #1792 closed. Its four native images were opened and inspected at their recorded size and enlarged scale: 8, 12 and 12/3/15 remain exact, without plus fallback, shared glyphs, clipping or chart overlap. The mixed counters are close together; their semantic colors distinguish the groups. These are observations of the recorded candidate, not new native captures after rebase.

Rebased onto `6bf0a4aeca2ec679c7c3523c4f6cb4abfa00a34a`, preserving main's Conversations/sidebar and M2 group-header decisions. `src/icon.rs` is byte-identical to the captured candidate; the native draw/raster selection and Dock call are unchanged. The code diff remains scoped to I2 exact counts and canvas fitting. R13 amends Product and M1's I2 reference; R8 retains the contract budget. The existing A2 matrix covers 0/9/10/12/99/100 in every counter position alone and together, both themes, and square/rectangular canvases. It keeps chart, palette, unknown/zero, blocked, pause and compact protections.

The resumed canonical gate `python3 scripts/check.py --base 6bf0a4aeca2ec679c7c3523c4f6cb4abfa00a34a` failed after 5.317 s / 900 s. Whitespace and selector/rule-gate self-checks passed. The newly adopted rule gate reports `changed M1, Product`, then refuses: `the task does not name M1 (changed), Product (changed)`. It did not reach Rust, Node or selftest stages. This is a contract authorization gap, not a sandbox failure or a passing gate. #1789 A1 explicitly names I2; an owner answer naming Product and M1 is required by R13 / [rule IDs](../../rule-ids.md#contract-detail) before acceptance can continue. No rule guard or test assertion was relaxed.

On the rebased renderer, `cargo +stable test --locked --bin taskq two_digit` passed both the complete counter matrix and native-canvas selection checks (2 passed, 636 filtered out; 0.67 s execution after an 85 s rebuild). `RUSTUP_TOOLCHAIN=stable sh docs/shots/update.sh` refreshed the isolated README captures from `8ba4675cc68fd5b47a952023399f8228f33f2175`; `sh docs/shots/check.sh` passed. Both output images were opened: the project capture remains unchanged, and the Summary retains all three counters and six chart columns. These snapshot observations do not extend the earlier native OS qualification.

`cargo +stable test --locked --bin taskq icon::tests` also passed on the rebased renderer: 6 passed, 1 subprocess helper ignored, 631 filtered out; 6.67 s execution. This retains palette, six-bin marks, unknown/zero, compact assets and first-capture behavior alongside the new counter matrix. The full canonical gate remains failed pending the R13 answer.

## Owner-approved continuation

Trusted owner answer `6493466267` explicitly authorizes Product and M1 alongside I2. The same branch remains based on `6bf0a4aeca2ec679c7c3523c4f6cb4abfa00a34a`; the implementation and source-pinned screenshots are unchanged. R13 governs the amended Product/M1/I2 decision; R8's size limit and R12's evidence/publication requirements are retained. No R-numbered principle body or test guard is changed.

The previously failing rule gate now accepts the same `changed M1, Product` diff and proceeds to npm/build/Rust checks, using the trusted answer. This resolves the explicit-ID failure without weakening the gate or editing the started task body.

On `7511ae8182abf955d9338bf967d757a1b8536f0a`, the approved canonical rerun `python3 scripts/check.py --base 6bf0a4aeca2ec679c7c3523c4f6cb4abfa00a34a` is **sandbox-blocked, not PASS**: 587 passed, 18 host-authority cases failed, 33 ignored; 112.612 s / 900 s. The R13 rule gate, R8 contract-size check, complete counter matrix and native-canvas test passed. The 18 failures again report worker restrictions on host installation, policy, recovery and Stop/restart, including `a worker cannot install the host service`, `a worker cannot change host policy` and `a worker cannot restart the host service or request Stop`. Integration and the later gate stages were not reached. No permission or assertion was relaxed; §7 leaves the host/platform acceptance gate to merge-result `merge_group` CI.

All 26 `tests/*.test.cjs` scripts and the three artifact scripts passed separately (29 commands, 49.463 s); commands match the earlier manifest plus `node tests/sync-view.test.cjs`. Isolated `TASKQ_INSTALL`, `TASKQ_SELFTEST_OFFLINE=1`, `TASKQ_INCIDENT_REPORTS=off` with `cargo +stable run --locked -- selftest` passed 99 golden traces and verdict formats and committed 9 staged observations; board access intentionally remains unknown. `sh docs/shots/check.sh` and `git diff --check` passed. Only this receipt changed after the tested candidate; implementation, tests, dependencies and screenshots remain unchanged.
