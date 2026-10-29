# Square summary icon

The [owner-approved design](../reviews/visual-decisions.md) supersedes the former quadrant histogram. [Contract](../../taskq.md#product) · [M1](menu.md#m1-appearance) · [R-UI4/R-UI5](rendering-contract.md#r-ui4-cadence-data).

## I1 Composition

One Rust SVG renderer owns the 22 pt-high Summary (minimum width 22 pt), its counters and chart group. Menu bar and running Dock rasterize it once at target scale; picker and History embed its vector groups. All four surfaces share the selected History period, default 24 hours. Icon remains the compact Q. [Custom is removed](custom-icon.md); saved Custom resolves to Summary, preserving unrelated settings.

Work = doing + review, Ask = ask, third count = ready + blocked. Hide each known zero; unknown is a dash. Preserve existing compact-Q fallback and pause/hold identity. Current counts are not event totals.

## I2 Type

System sans serif, weight 500, nominal 9 pt, baseline y = 9. Show 1–99 exactly and 100 or more as `99+`. Align measured glyph ink from preferred origins x = 1, 8.1, 15.4; retain ordinary lining numerals and native font curves. Preserve those origins when the ink fits. Reclaim hidden-zero cells before widening the menu-bar slot; keep at least 1.5 pt between visible counters and 1 pt at either edge. Never shrink menu-bar type or overlap counters or the chart. Dock fits the complete shared composition into its square canvas. Tooltip/menu counts remain exact.

## I3 Geometry

Exactly six separated columns, oldest left. For i = 0…5: x = 1 + 3.5i, width 2.5, gap 1, baseline y = 20.5, maximum height 9 (logical points). Height = 9 × closed / peak, where peak is the maximum closed count over complete-coverage bins, ongoing included. Unknown bins never set peak; peak zero is safe. Failed, ask and blocked never change height.

## I4 Marks and palette

- Known zero: neutral tick at y = 20, height 0.5, column width.
- Unknown/incomplete source: neutral mid dash at y = 15.5, height 0.75; no guessed green bar.
- Bin containing now: separate neutral underline at y = 21, height 0.5; no underline in a completed historical range.
- Known failed > 0: fixed red cap at y = 10.5, height 0.75, independent of green height.
- Activity history alone reveals a fixed Ask cap at y = 12, height 0.75, column width, in the Ask color when observed Ask > 0 (including incomplete bins). It remains hidden in menu-bar, Dock and picker icons. No blocked marks.
- Palette A light/dark: closed/work `#087F5B` / `#38E8A0`; ask `#995200` / `#FFC247`; failed `#C92641` / `#FF6378`; neutral `#536171` / `#B9C6D8`. One source supplies counters, graph and menu CSS. Board label API colors stay separate.
- Paused: existing gray composition and centered compact Q/pause identity. Partial pause does not gray everything.

## I5 Raster and acceptance

Reference geometry scales to the actual menu-bar height, then its backing scale; slot width follows I2. Dock renders at 256 px. Picker uses the vector at 18 CSS px. History projects only the canonical chart group to 320 × 88 logical px at base menu width, x × 16 and y × 8; layout adapts with width/font, never bins. Its CSS reveals the canonical SVG’s hidden Ask marks without changing Closed geometry or peak; failed, unknown, zero-Closed and ongoing marks remain distinct.

Use the frozen day/hour fixtures linked from the approved review, plus explicitly synthetic zero-peak, failed and unknown cases. Checks retain bin identities, exact counts, heights, colors and marks at every viewport. Inspect at most ten native screens for owner acceptance: font/DPR, separated columns, counter bounds, zero vs unknown, ongoing/failed, pause and overflow. Static references and source CI are not native acceptance.

## I6 Cross-platform rendering

Share vector geometry, data and palette; calibrate SF/Segoe ink and actual OS slot/DPI separately. Preserve Windows availability until native qualification. No repeated PNG resizing, width rebinning or dropped bins. Native/runtime gaps remain unknown under R12.
