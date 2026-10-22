# Visual decisions: one icon renderer

**Status:** final design approved by the owner on 2026-10-22 (`1, принимаем`). Palette A, plain columns A, review 05 geometry/state cues and Custom removal are approved. This is the implementation handoff package; native qualification remains separate. Research only; no source, canonical design specification, service or live installation changed. Branch `codex/visual-review`, base `ccd33b88`.

## Owner decisions

| Decision | Chosen option |
|---|---|
| Renderer | One SVG renderer for menu-bar Summary, running Dock icon, appearance picker and menu History. |
| Bins | Exactly six equal time bins for every surface and every period; duration = period / 6. No width-dependent bin count or rebin. |
| Shared palette | One semantic palette for graph, icon counters and menu CSS tokens. Palette A approved. |
| Quantity | Closed events only determine green column height. Failed does not increase green height. |
| Encoding | Ordinary separated solid columns, A in review 04. Constant green; common zero baseline and a linear scale to the window maximum. Heat opacity and six fixed cell outlines are rejected. |
| Other event kinds | Separate failed indication. Ask/blocked have no column marks or height contribution; all four exact counts remain in History details. |
| History selection | Each of six bins is selectable; reveal exact closed/failed/ask/blocked event counts and its time interval. |
| Custom | Remove CLI-only `taskq tray --icon FILE` and user-defined icon schema. Keep Icon and fixed Summary. Previously selected Custom falls back to Summary; preserve unrelated settings. |
| Evidence | Use real TaskQ snapshots and recorded events. Explain time, count and visual meaning. Do not invent activity to make a candidate look better. |

The owner explicitly superseded the initial heat-strip choice: quantity must be readable by height. Palette A, six bins and separate exact detail survive that correction. Column A was approved first; the subsequent final review explicitly approved its geometry and zero/unknown/partial/failed rules below.

## Approved palette

| Semantic token | Light | Dark |
|---|---|---|
| Work / closed | `#087F5B` | `#38E8A0` |
| Ask | `#995200` | `#FFC247` |
| Failed | `#C92641` | `#FF6378` |
| Neutral: Ready / Blocked / Review / Later / Held | `#536171` | `#B9C6D8` |

These are semantic colors, not a change to board-label API colors. Preview surfaces `#FAFBFC` / `#252B32` and edge colors are context, not newly approved background tokens. All 16 semantic text/surface pairs in palette review 01 met 4.5:1. Native translucent backgrounds still need qualification.

## Real data and provenance

- Frozen capture: **22 October 2026, 21:16:25 UTC+7**. [Whitelisted seven-board fixture](real-data-2026-10-22.json); [derived TaskQ hour](real-data-hour-2026-10-22.json). Status and event files were read only. Current observations are not historical queue snapshots.
- TaskQ source: `<projects>/taskq/.taskq/events`, board `https://github.com/taskqorg/taskq/issues`. [All 39 closes, linked by bin](height-review-04.md#verified-taskq-source). Frozen and reread event SHA-256 matched; all 72 window records remained present. GitHub independently corroborated recent closed issues #1586 and #1588. This is not a claim that every issue was freshly fetched.
- TaskQ 24-hour candidate grid: 21 Oct 23:00 → 22 Oct 23:00, six whole epoch-aligned 4-hour bins. Closed **5 / 4 / 7 / 12 / 7 / 4**; peak 12, total 39. Last bin is ongoing and only observed to 21:16:25. Lower current-bin volume does not establish a slowdown.
- Real zero fixture: 20:20 → 21:20, six 10-minute bins, closed **1 / 0 / 0 / 1 / 0 / 0**. The two closes are #1586 and #1588.
- Real incomplete-source fixture: PilotQ collection starts today at 15:48:24. First five aggregate 4-hour bins have incomplete coverage; their observed closes are lower bounds **≥6 / ≥8 / ≥9 / ≥12 / ≥11**. The sixth has 6 recorded closes so far.
- Frozen current counts: Doing 1, Review 0, Ask 0, Failed 0, Ready 1, Blocked 0, Later 0, Held 3. Summary is Work 1 / hidden Ask zero / Ready + Blocked 1. Held remains menu-only. The single-board illustration does not change the icon's all-observed-board scope.
- No failed event exists in the seven available caches. Do not substitute `gone` for failed. Failed-marker runtime readability remains unqualified. Collection-start metadata does not prove uninterrupted collection; cached zero is not proof that no event occurred during an unrecorded gap.

## References

- **Approved style A:** [dark](img/height-review-04-dark.svg.png), [light](img/height-review-04-light.svg.png); [source proof and comparison](height-review-04.md). B is rejected.
- **Approved surface review 05:** [dark PNG](img/surface-review-05-dark.svg.png), [light PNG](img/surface-review-05-light.svg.png); editable [dark SVG](img/surface-review-05-dark.svg), [light SVG](img/surface-review-05-light.svg). Two sheets × five screens = ten screens: non-notch menu bar, notch menu bar, Dock, picker with real data states, History, each theme.
- Review 01 color choice remains valid. Reviews 02/03 heat encodings are superseded and are not final references. Synthetic zero/failed examples from review 02 are not production observations.

The review images retain their original “proposed” captions; the owner approved those exact bytes. The approval above, not a redraw, changes their status.

| Approved artifact | SHA-256 |
|---|---|
| `surface-review-05-dark.svg` | `4d9073f8ad845f008a298e6afa3fa47e2b03dec81835d4e453738e1bb42f8139` |
| `surface-review-05-dark.svg.png` | `72d742f9e7bf0c86cc4cc4e9683d324468eddcde4ba5b792bcff0b0832bed3a5` |
| `surface-review-05-light.svg` | `a9bbe3c5b5a092014ca038daa8b1db47c64d0251c2b024dabb003942a62bd8f1` |
| `surface-review-05-light.svg.png` | `ec247277adf2f0787d16856fc8bccaa4c34a01f9b15ae35695fc7c1c1c515d58` |

## Approved geometry

Coordinates are logical points of a 22 × 22 reference, origin top left. Ordinary system sans serif, requested weight 500. Previews use Helvetica fallback; native SF/Segoe ink metrics must be calibrated before implementation acceptance.

| Element | Reference geometry |
|---|---|
| Counter row | Work, Ask, Ready + Blocked, left to right. All use 9 pt nominal type and baseline y = 9 pt; reference text origins x = 1, 8.1, 15.4 pt. Pin measured glyph ink to the assigned cells, not incidental side bearings. |
| Count policy | Hide each known zero without moving other positions; unknown remains a dash. Current meanings do not change. Existing overflow and compact-Q fallback behavior are preserved; no new threshold, shrinking or widening policy is approved here. |
| Green columns | For i = 0…5: x = 1 + 3.5i pt, width 2.5 pt, gap 1 pt, baseline y = 20.5 pt, maximum height 9 pt; oldest left. |
| Height | h(i) = 9 × closed(i) / peak, peak = max closed over bins with complete source coverage, including an ongoing bin's observed count. Unknown values never enter peak. Known counts are nonnegative. |
| Known zero | Neutral baseline tick, x as column, y = 20 pt, width 2.5 pt, height 0.5 pt. A tick is not a minimum-height green bar. If peak = 0, every known-zero bin is a tick; no division by zero. |
| Unknown / incomplete source | Neutral dash halfway up: y = 15.5 pt, width 2.5 pt, height 0.75 pt. Do not paint a guessed green height. Preserve observed lower-bound counts and the missing source in detail. |
| Ongoing time bin | Separate neutral underline at y = 21 pt, width 2.5 pt, height 0.5 pt, in the bin containing now. No underline in a completed historical/custom range. It can coexist with a known zero or unknown dash. |
| Failed presence | Fixed red cap at y = 10.5 pt, width 2.5 pt, height 0.75 pt in each bin with known failed > 0. Presence only, not magnitude; independent of green height. Never infer no failure from unknown history. No invented failed marks in the real-data references. |
| Menu bar | Scale reference geometry to the owning native status-item slot once, then render at its backing scale. Review shows 22 pt and 33 pt; 33 is a notch example, not a hardcoded rule for all notch displays. |
| Dock | 256 px running Summary, rendered from the same canonical geometry. Column count and gap proportions stay fixed. Static not-running compact-Q assets retain their current identity. |
| Picker | 18 CSS px, vector rendered at native DPR; no intermediate 44 px bitmap resized to 18. The 44 px example is an inspection aid only. |
| History | Base menu width 360 at 16 px type; plot spans 320 logical px. Same six normalized column paths and state marks; the approved view maps canonical chart x by 16 and y by 8, so peak height is 72 px. Layout adapts with menu width/font; bin count never does. Counters above the icon are omitted from the History viewport. |

The chart group has one canonical source. Cropping/projecting it into History changes its viewport, not binning, quantities, palette or normalized heights. Conversion to the final device scale happens once; implementation must snap shared straight fill boundaries without changing six-bin semantics. Paused/hold/unknown admission state keeps its existing identity; changes to these policies are outside this visual decision.

## Approved History detail

- Clicking or keyboard-selecting a bin updates detail inside the same History panel; no extra page. Keep the selected bin across refresh by time identity. A selected-bin focus outline is not an outline around all six graph cells.
- Detail names the exact half-open time interval `[start,end)`, timezone, closed, failed, ask, blocked, and whether the bin is ongoing or has incomplete source coverage. Unknown values are not zero; known observed counts may be shown as lower bounds with their missing source.
- Legend retains all four event totals with completeness qualifiers. It is distinct from current task counters. Do not display an exact complete aggregate when a contributing source is missing.
- The source period defaults and custom range validation persist. Presets step whole epoch-aligned bins of period / 6; exact custom bounds stay exact. Partial current time and missing history are distinct states.

## Review evidence and limitations

- Pass: both approved final PNGs opened and visually inspected. All ten screens, six-bin shapes, current-counter positions, real zero/unknown examples, time labels and selected detail are visible without clipping. A label overlap found during export was fixed and both PNGs inspected again.
- Pass: fixture assertions reconcile all six TaskQ bins with their recorded events and reconcile the derived hour with its two source issues. Equal 7 counts have equal height; 4 has one-third of the peak-12 height. No opacity quantity encoding remains.
- Not tested: native OS rendering, SF/Segoe calibration, translucency, Dock downscaling, actual notch geometry, Windows slots, clicks/keyboard, multi-digit overflow, pause, fully zero peak and failed > 0. Static references are not native or runtime proof.
- No source/canonical design edits, deployment, task filing or service changes. Final owner approval authorizes committing this package, followed by the empty `VISUAL_DONE` handoff marker. Custom icon removal does not remove custom History time ranges.
- Validation: SVG XML parsing, PNG dimensions, relative document links, fixture/event count reconciliation and staged whitespace checks. Full `scripts/check.py` build was not run: this research-only scope permits writes only under `docs/reviews/`, whereas that gate installs/builds product artifacts outside it. No runtime-test result is claimed.

## Task 1: one icon renderer

### Goal

Menu-bar Summary, running Dock icon, in-menu picker and History show one consistent six-bin, height-based closed-event graph from one SVG renderer and one palette. Remove Custom and its schema. Preserve current task meanings, admission state and unrelated settings.

### Acceptance

1. **Spec first:** amend taskq.md as required and its R-UI4/R-UI5, icon-summary I1–I6, menu M1/M6, custom-icon and related references before implementation. Supersede heat intensity, stacked-height semantics, twelve source bins and width-dependent rebinning with six plain closed-height columns, the approved palette, geometry and zero/unknown/partial/failed rules. This review stays research until those canonical changes land.
2. One Rust SVG renderer owns the Summary composition and chart group. Menu bar and Dock rasterize the SVG once at the target scale; picker and History embed the same canonical vector groups. History may crop/project the chart viewport and retain selectable bin IDs. No JS/DOM chart reconstruction, separate geometry semantics or per-surface rebin. Delete `historyRaster`, old `icon::history_rows` and duplicate color tables when replaced.
3. Exactly six source bins for every preset and custom period, duration = period / 6; preserve event identity, deduplication, half-open bounds, rolling whole-bin alignment, last-bin partial state, all four exact event counts and completeness. Current task counters remain separate.
4. Closed only sets green height; shared linear zero baseline and per-window peak. Failed is separately indicated, ask/blocked remain in exact detail. Unknown is never zero or used to invent a peak; zero peak is safe. Palette A supplies graph, counter and CSS semantic tokens. Preserve compact Q, pause/hold identity, current overflow behavior and all unrelated routes unless separately approved.
5. History has six keyboard-accessible selectable bins. Detail and legend show exact or explicitly incomplete values, interval/timezone and ongoing/source coverage. Selection does not navigate away or alter scope, admission or period.
6. Remove `tray --icon FILE`, Custom configuration validation/rendering, saved-custom layout selection and editor/prototype hooks that no longer have a supported caller. Retain Icon and fixed Summary constants; previously selected Custom resolves to Summary without deleting unrelated settings. Diagnose an obsolete CLI flag clearly. Update affected docs, tests, exports and dependencies; remove a dependency only if unused elsewhere.
7. Reuse the frozen real day/hour/incomplete-source fixtures for comparison on all surfaces. Test canonical bin IDs, counts, colors, normalized heights and state marks across renderer viewports. Add only necessary distinct logic cases for zero peak, known failed and unknown input; label synthetic regression cases explicitly. Source tests do not substitute for visual review.
8. Supply owner-review images for light/dark and relevant native menu-bar, Dock, picker and History sizes. Qualify actual fonts/DPR, six separated columns, no cropped/colliding counters, zero vs unknown, ongoing and failed cues, pause and existing overflow. Preserve Windows availability rules until its native qualification; do not claim this static package as runtime acceptance.

### Scope

Implementation: `src/icon.rs`, `src/tray.rs`, `src/tray-app.js`, `src/tray.html`; other CLI/settings callers of `--icon` and Custom; `Cargo.toml` only for the SVG rasterizer or genuinely unused Custom dependencies; affected tests/fixtures. Canonical docs: `taskq.md`, `docs/design/rendering-contract.md`, `icon-summary.md`, `menu.md`, `custom-icon.md` and affected links/presets/prototype scripts. Coordinate with #1578 because it edits the tray UI. Do not widen into queue, worker, service, installation or task-state changes. The parallel implementation/review session owns task filing and code changes.
