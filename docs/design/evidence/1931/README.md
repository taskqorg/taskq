# Summary A implementation — #1931

The shared Rust SVG now stacks Completed, New, Ask and Failed on one linear event scale. Six chronological bins, current Work / Ask / Ready+Blocked counters, exact 1–99/99+, native numeral outlines, slot widening, compact Q and pause behavior are retained. Blocked remains exact History detail. Unknown New never erases known legacy completion/question history or becomes a complete zero.

## Candidate and authority

Implementation: `c9af254b50631aa8b552005957b32195d255dbcc`, rebased once onto `04e43344db6dafcd94c46e06ecd13d908ebcefa2`. Subsequent changes contain this evidence and the native capture instrument's paint barrier; shipped sources are unchanged. No installation, service restart, permission change, real task admission or PM message was performed.

R13 applies the selected I3/I4 target and owner answers `6524988858` (M6), `6525408616` (M1), plus steer `6525411430`. Amended rules: M1, M6, I3, R-UI4 and the menu Contract detail's History source. I1/I2, R1/R8/R9/R12 and the #1789 counter decisions remain protected. No R-numbered principle body changes.

The v2 packet SHA-256 was verified as `e397147ee8fe6d6867af8ecfe604141e9921b0ce6c46e2f059a03851e9b3342d`; its entry, plan, acceptance, issue map and selected-A reference were inspected. Its counter reversal and B/C alternatives are not implemented.

## Data and compatibility

[I3](../../icon-summary.md#first-admission-source-and-compatibility) owns the source, schema and rollback. A read-only collector uses each trusted issue's earliest recorded `taskq` label addition, including first Held admission and manual admission. Closed and label-removed issues are included. Identity is board + issue + event ID, with the original board time. Retries, resumes, requeues and later label additions do not create another arrival. The collector never blocks the service heartbeat/admission loop.

`incoming` schema 1 is an optional projection in the existing `status.json`. Old readers ignore it; absent, corrupt, unsupported or foreign projections are unknown. The original four-kind `events` file is unchanged. Cache loss rebuilds from recorded board events, never creation dates/current queue counts. A failed scan or dead collector retains known lower bounds with explicit incomplete coverage. Rollback ignores the projection and preserves original files and board history; no task database or board log format is added.

API references: [GitHub issue events](https://docs.github.com/en/rest/issues/events#list-issue-events), [GitLab resource label events](https://docs.gitlab.com/api/resource_label_events/). One live read of #1931 verified GitHub label event `34694958160`, `2026-10-30T13:34:09Z`, `labeled`, `taskq`. This confirms the source record, not a deployed collector or a live cross-host restart flow. GitLab parsing is fixture-tested; its live endpoint and cold full-history scan cost are Not tested.

## Verification

| Check | Observed result |
|---|---|
| Before: `cargo test --locked --bin taskq selected_event_stack_uses_quantities_on_every_surface` | FAIL: Completed 2 + Ask 3 + Failed 1 produced peak 2, expected 6 |
| After: the same check; `cargo test --locked --bin taskq icon::tests` | PASS; 6 passed, 1 subprocess helper ignored. Peak 6; adding New 1 gives 7. Exact geometry, rare subpixel segments, overflow-safe sums, palette, empty Q, pause and #1789 counter/canvas bounds retained |
| `cargo test --locked --bin taskq incoming` | PASS, 4 cases: restart/cache loss, first Held/manual admission, retries/runtime changes, closure/label removal, missing/corrupt/foreign sources, complete zero, half-open boundaries, cross-host deduplication and nonblocking/dead collector |
| `cargo test --locked --bin taskq admission_tests` | PASS, 2 provider parsers; earliest addition only, malformed time rejected, empty event list distinct |
| `cargo test --locked --bin taskq approved_real_history_fixtures_use_one_six_bin_svg` | PASS; frozen day/hour and original exact event counts retained; legacy New unavailable; synthetic stack/gap/rare/overflow fixtures added. Duplicate-source failure fixed by deduplicating missing reasons without changing its assertion |
| `node tests/tray-history.browser.cjs` | PASS, 72 cells: 12 scenarios × 2 themes × DPR 1/2; plus 3 widths × 4 fonts × DPR 1/2. Six bins, canonical SVG identity, exact/lower-bound detail, keyboard selection, refresh identity, no horizontal overflow, widened counter viewport and pause retained |
| `python3 scripts/check.py --base 04e43344db6dafcd94c46e06ecd13d908ebcefa2` | **sandbox-blocked, not PASS**; R13 gate accepted changed M1/M6; 714 unit tests passed, 35 ignored; integration 20 passed, 1 blocked, 1 ignored. 209.087 s / 900 s; no retry |
| Blocked integration | `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. No permission or assertion was relaxed |
| Skipped gate stages run separately | All 29 `tests/*.test.cjs` scripts plus 3 artifact scripts passed: 32 commands, 60.714 s. Isolated offline selftest: 113 golden traces, formats and 9 committed observations PASS; board/tools intentionally unknown |
| Native WebKit instrument | PASS: 14 frozen views at 360 × 800 logical / 720 × 1600 backing pixels, macOS 27.0.1 arm64. Final representative frames opened and inspected |
| Native TaskQ snapshot adapter | PASS: isolated fixture installation, actual menu-bar geometry 34 pt @2x, final 68 × 68 Summary export. Natural-size glyphs/columns inspected; no bitmap resizing stage |
| Actual shell presentation | **Not tested:** visible menu-bar placement, live Dock presentation, other physical displays/scales, native Windows notification area/Segoe/readability, screen readers and integrated owner review |

The browser instrument used Playwright 1.64.1 and its headless Chromium shell. Tool dependencies were isolated from the repository lockfile. Full gate was run once after rebase; remaining stages were run separately after its sandbox refusal. Publication still needs green merge-result `merge_group` CI (R12, §7).

The previous Closed-only/hidden-Ask/fixed-mark assertions were replaced under the approved M1/M6 change. The retained protections cover source identity, exact original event counts, stable time bins, missing coverage, type/collision bounds, compact identity, palette and pause. No check was weakened to hide an implementation failure.

## Visual review set

Six images; no theme/size-only variants. All activity in the screenshots is frozen or explicitly synthetic, not a live productivity measurement.

1. [Four quantitative stacks](01-stacks.png): canonical green/blue/amber/red order, equal bins, exact Blocked detail.
2. [Legacy and partial sources](02-legacy-sources.png): unavailable New and observed lower bounds remain explicit.
3. [One incoming interval gap](03-interval-gap.png): third bin hatched, excluded from scale, other stacks retained.
4. [Overflow picker](04-overflow-picker.png): shared vector retains three exact counter groups and the 99+ cap; its 18px preview is dense and does not qualify menu-bar readability.
5. [Canonical before/after](05-before-after.png): original baseline SVG, same recorded day under A, and labelled four-kind illustration. These are renderer exports, not OS shell screenshots.
6. [Actual slot-sized export](06-native-slot.png): 34 pt @2x from TaskQ's native snapshot adapter; 68 × 68 pixels. This is an exported icon, not a capture of its placement in the OS shell.

The initial offscreen WebKit instrument sometimes captured the previous SVG paint after the text had updated. A partial-New screenshot exposed the mismatch. The final instrument uses a visible isolated window, waits for two animation frames, records DOM unknown-mark counts and has a 30 s deadline. The final partial case has one DOM unknown mark and the visibly hatched third bin. This repairs the capture instrument; it does not assert an application rendering defect.

## Acceptance map

| Item | Result and limits |
|---|---|
| A1 | PASS: #1789 and I1–I6/R-UI4–5 reconciled; M1/M6 aligned under explicit answers; no counter reversal or width/type rollback |
| A2 | PASS: shared event scale/order, six equal chronological bins, exact Blocked detail; canonical fixtures and browser/WebKit output |
| A3 | PASS at model/adapter-fixture boundary, plus one live GitHub source read; deployed cross-host/restart collection Not tested |
| A4 | PASS: legacy missing New unknown, known old series visible, interval gaps distinct from complete zero, retained lower bounds |
| A5 | PASS: zero ticks, detached caps and ongoing underlines removed; no minimum colored segment height; event/productivity distinction explicit |
| A6 | PASS at source/browser/WebKit boundaries: one Rust SVG and data projection; compact assets and native font paths retained; no study renderer shipped |
| A7 | Logic, browser matrix, native WebKit and actual Mac slot geometry PASS; OS-shell presentation/Windows/accessibility and integrated native acceptance Not tested |

Native integration remains for #1932/#1933. No release or rollout is claimed.
