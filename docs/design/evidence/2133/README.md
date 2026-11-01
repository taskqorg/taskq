# Summary counter order — #2133

Summary now reads Ready+Blocked / Ask / Work from left to right. Authority: [owner request and acceptance](https://github.com/taskqorg/taskq/issues/2133). Amended decisions: R13 Product and I1/I2/I6. Clean baseline: `8d75d224133cd18aba592a03031b0da597dcd28b`; implementation: `8b2ba9b1` (the only subsequent changes are this evidence). The baseline already had no reversal-under-review wording in these canonical sections.

Only the presentation tuple in `src/icon.rs::svg` changes. `tray::summary_at` keeps its Work/Ask/Ready/Failed source array, Work remains doing+review, and Ready+Blocked retains its saturating sum and unknown propagation. Menu bar, Dock and picker consume this shared SVG; History projects its unchanged chart group. Exact tooltip text/values, menu task-filter order, event arrays, six oldest-left/newest-right bins, palette and settings remain unchanged.

## Before and after

`cargo test --locked --bin taskq summary_counter_order_preserves_values_and_colors` failed against the old renderer: `cell 0 must show 3 with its semantic color`. After the tuple reversal, `cargo test --locked --bin taskq icon::tests` passed: 7 passed, 1 ignored subprocess helper. The new check protects positional values, semantic colors, Ready+Blocked addition, zero omission, unknown propagation and 99+ overflow in both palettes. The existing geometry test now maps its cell index to the reversed source index; its ink bounds, spacing, chart separation, hidden-zero reclamation, native slot widening and pause assertions are retained. Existing event/compact-asset checks passed unchanged.

## Visual review

After the one rebase, the new check exported the canonical SVG and `raster_slot` PNGs with `TASKQ_ICON_EVIDENCE` set to a temporary directory; the existing `approved_real_history_fixtures_use_one_six_bin_svg` exporter used a separate temporary directory and passed. No older dirty checkout or study renderer was used.

| Fixture | Light | Dark | Observation |
|---|---|---|---|
| Distinct Ready+Blocked=3, Ask=2, Work=1 | Pass | Pass | Neutral 3, amber 2, green 1; separated native SF glyphs |
| All known zero | Pass | Pass | No counters; existing compact Q fallback |
| All unknown | Pass | Pass | Three dashes with their original semantic colors |
| Ready+Blocked=100, Ask=99, Work=100 | Pass | Pass | 99+ / 99 / 99+; widening retains complete glyphs |
| Existing four-kind history | Pass | Pass | Current 15 / 3 / 12 above six separated stacked bins |

Counter exports were opened at 256 px reference height; overflow was also opened at 44 px height (the slot expands horizontally). Both existing stack exports were opened at 44 px. The existing paused dark export was opened at 44 px: gray data and centered Q/pause identity remain. These are canonical renderer exports with native macOS SF outlines, not OS-shell captures. Automated geometry additionally covers 22/44/256 px raster targets and every zero/nonzero cell mask.

No native menu-bar placement, running Dock, picker interaction, Windows/Segoe/DPI, Linux desktop or accessibility qualification was performed. Those claims remain unknown under R12; this task authorizes no install, restart or settings change.

## Candidate gate

`python3 scripts/check.py --base 8d75d224133cd18aba592a03031b0da597dcd28b` ran once, in 207.636 s / 900 s: **sandbox-blocked, not PASS**. Rule gate accepted Product. Rust unit checks: 834 passed, 40 ignored. Integration: 22 passed, 1 sandbox-blocked, 1 ignored. The blocked case is `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. No assertion, permission or sandbox policy was changed. Contract lint was unknown because the checker's pinned specq checkout was unavailable locally; the supplied task-rule cache also lacks its lint script. CI must run that lint.

The unexecuted remainder ran separately, without repeating the full gate: isolated offline selftest passed 113 golden traces, formats and 8 committed telemetry observations; all 37 Node component scripts and four artifact checks passed. The remainder took 92.783 s. Offline board/tool probes intentionally remain unknown. `TASKQ_ICON_EVIDENCE=<fresh-fixture-directory> node tests/tray-history.browser.cjs` passed all existing scenarios, themes/DPR, exact/incomplete detail, keyboard selection, refresh identity and Custom removal against the newly generated canonical SVGs. No fixture data or event-series baseline was rewritten.

R12/§7 require reviewer publication with green merge-result `merge_group` CI; worker hand-in does not establish merge or deployment.
