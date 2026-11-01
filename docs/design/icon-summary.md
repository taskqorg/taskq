# Square summary icon

The [UI30 target](ui-polish.md) selects event stack A over the earlier closed-only encoding. The canonical renderer implements it; native qualification remains separate (R12). [Contract](../../taskq.md#product) · [M1](menu.md#m1-appearance) · [R-UI4/R-UI5](rendering-contract.md#r-ui4-cadence-data).

## I1 Composition

One Rust SVG renderer owns the 22 pt-high Summary (minimum width 22 pt), its counters and chart group. Menu bar and running Dock rasterize it once at target scale; picker and History embed its vector groups. All four surfaces share the selected History period, default 24 hours. Icon remains the compact Q. [Custom is removed](custom-icon.md); saved Custom resolves to Summary, preserving unrelated settings.

Left to right: Ready+Blocked = ready + blocked, Ask = ask, Work = doing + review. Hide each known zero; unknown is a dash. Preserve existing compact-Q fallback and pause/hold identity. Current counts are not event totals.

## I2 Type

System sans serif, weight 500, nominal 9 pt, baseline y = 9. Show 1–99 exactly and 100 or more as `99+`. Align measured glyph ink for Ready+Blocked / Ask / Work from preferred origins x = 1, 8.1, 15.4; retain ordinary lining numerals and native font curves. Preserve those origins when the ink fits. Reclaim hidden-zero cells before widening the menu-bar slot; keep at least 1.5 pt between visible counters and 1 pt at either edge. Never shrink menu-bar type or overlap counters or the chart. Dock fits the complete shared composition into its square canvas. Tooltip/menu counts remain exact.

## I3 Geometry

Exactly six separated columns, oldest left. For i = 0…5: x = 1 + 3.5i, width 2.5, gap 1, baseline y = 20.5, maximum height 9 (logical points). Stack, bottom to top: Completed, New, Ask, Failed. Height = 9 × the sum of the shown event counts / peak, where peak is the maximum corresponding sum over complete-coverage bins, ongoing included. These are events, not unique tasks or present queue size. Blocked event counts remain in exact History detail.

New means first admission of a task into the work queue; held drafts count only when first admitted. Retries, resumes, requeues and runtime changes do not create another New event. Preserve board/task identity and durable provenance; current queue size or issue creation time cannot substitute for this event. Legacy ranges without incoming history keep New unknown.

If a series was not collected anywhere in the range, retain the other known event series, label the missing series explicitly and hatch the known stacks as incomplete. Scale only the available kinds over bins complete for those kinds. Do not replace unavailable New with zero or hide known completions/questions. A per-interval collection gap uses an unknown slot and retains known lower bounds in detail; it does not set peak. Peak zero is safe. Never impose a minimum colored height that exaggerates rare events.

### First-admission source and compatibility

New is the earliest recorded addition of the `taskq` eligibility label on a trusted issue, read from the board's complete paginated label-event history. This includes the first Start of a Held draft and manual label admission. Later removals/additions, retries and runtime changes keep that first identity. Use board URL + issue number + label-event ID, with its original board timestamp. Never use issue creation or the time a host first saw a task.

The service rebuilds a rolling 30-day incoming projection from updated issues (closed and label-removed issues included), then reconciles updates with an overlap. Collection runs on a read-only thread; the service loop never waits for label history. Only a completed scan establishes coverage. Missing provenance, unreadable events and failed scans leave New incomplete; retain observed lower bounds. Old ranges without a label-event record stay unknown. A successful rebuild may recover recorded events, never synthetic backfill.

Resume the board-matched `status.json incoming` read cache after restart, including settled observations, unresolved sources and the update-list cursor. Advance that cursor after each successful list independently of complete coverage, so one failed source cannot relist the month. Read at most five issue event histories per collector pass, retrying unresolved sources without starving unvisited ones. Each source still requires its complete paginated history. A label event without a label name (the label was deleted) admits nothing. A complete history whose admission record is malformed (no ID, no time or invalid provenance) is read again only after its issue updates; until then it stays a named missing source and coverage stays incomplete. Request failures and absent history stay pending and retry within the five-read bound. Expire observations outside the rolling month; a missing, incompatible or corrupt cache rebuilds from the board with incomplete coverage.

`status.json` carries optional `incoming` schema 1: board identity, covered `[start,end)`, events and missing-source reasons. It is a rebuildable read cache, never queue state. Older readers ignore it; newer readers treat absent/unknown versions or mismatched board identity as unavailable. Original `events` bytes and its four-kind order remain unchanged. A failed collection/save retains the last known events with bounded coverage and a reason; rollback ignores this projection without deleting source history. No new task database or board format is introduced.

## I4 Marks and palette

- Known zero: empty chart space; no minimum-height bar or baseline tick. Preserve the compact-Q fallback when current counters and all known history are empty.
- Unknown/incomplete source: explicit hatching and source/series explanation; never a guessed zero. Retain known event segments when a whole series is unavailable.
- Bin containing now: position and exact interval/unfinished status in expanded History; no separate underline in the small icon.
- Failed and Ask contribute their quantitative red/amber stack segments on all surfaces; no detached caps or History-only Ask encoding. Exact Blocked counts remain in History detail, without a new small-icon series.
- Palette A light/dark: closed/work `#087F5B` / `#38E8A0`; ask `#995200` / `#FFC247`; failed `#C92641` / `#FF6378`; neutral `#536171` / `#B9C6D8`. One source supplies counters, graph and menu CSS. Board label API colors stay separate.
- New event accent: blue `#1F65C5` / `#6EA8FF`; it means incoming events, not the present Ready/Blocked counter.
- Paused: existing gray composition and centered compact Q/pause identity. Partial pause does not gray everything.

## I5 Raster and acceptance

Reference geometry scales to the actual menu-bar height, then its backing scale; slot width follows I2. Dock renders at 256 px. Picker uses the vector at 18 CSS px. History projects only the canonical chart group to 320 × 88 logical px at base menu width, x × 16 and y × 8; layout adapts with width/font, never bins. The target projects the same event stack and coverage on every surface; History adds exact interval, ongoing and source details, rather than revealing a different Ask encoding.

Use the frozen day/hour fixtures with unavailable New and retained known series, plus synthetic four-kind, zero-peak, failed and unknown cases. Checks retain bin identities, exact counts, heights, colors and marks at every viewport. Inspect at most ten native screens for owner acceptance: font/DPR, separated columns, counter bounds, zero vs unknown, ongoing/failed, pause and overflow. Static references and source CI are not native acceptance.

## I6 Cross-platform rendering

Share vector geometry, data and palette; calibrate SF/Segoe ink and actual OS slot/DPI separately. Preserve Windows availability until native qualification. No repeated PNG resizing, width rebinning or dropped bins. Native/runtime gaps remain unknown under R12.

Counter order is Ready+Blocked / Ask / Work; history remains oldest-left/newest-right. Keep I1/I2 exact 1–99 and 99+, hidden-zero space reclamation and native slot widening from #1789. The older snapshot’s fixed square and 9+/+ wording are not adopted.

Implementation checks and remaining native gaps: [#1931 evidence](evidence/1931/README.md).
