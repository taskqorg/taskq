# TaskQ rendering contract

Target for native implementation. Apply [shared F1–F6](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md).
[Menu](menu.md), [icons](icon-summary.md), [visual tokens](visual-style.md), [AI](ai-workspace.md) remain semantic authorities.

## R-UI1 Reference ownership

- `final-prototype.html`: interaction/layout candidate, not production code.
- Approved composition and feature decisions survive implementation. Candidate demo shortcuts do not: fixed demo heights, sample totals, synthetic links, simulated windows, static permission details.
- Compact q assets and I1–I5 own icon identity. Use ordinary system numerals; no seven-segment substitute. Match ink bounds and pause overlay, not a rough silhouette.
- Each acceptance report applies [F1](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md#f1-source-and-change-control). Implementation must identify pending I5 decisions instead of silently selecting an overflow or bin policy.

## R-UI2 Native menu sizing

Apply [F2](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md#f2-size-to-content-then-constrain). Logical units below. Convert physical monitor/anchor values exactly once using that monitor's scale.

- Windows: monitor containing the tray anchor, usable rectangle from the native work-area API (including taskbar). macOS: owning display's visible frame and menu-bar anchor; normalize coordinate origins.
- Keep 8 logical units at horizontal usable edges. Prefer a 4-unit anchor gap when it fits. Vertical height may use the full usable work area; reserve only native chrome/shadow or OS-required clearance, counted once.
- Base menu width: 360 at 16 px. Target `max(320, 360 × font / 16)`, clamped to usable width minus margins. Thus 18→405, 20→450 when space permits. Screen constraints may reduce below 320; reflow, never overflow off-screen.
- Prefer below a top/menu-bar anchor and above a bottom/taskbar anchor. Try the opposite side if needed, then slide vertically within the full work area before constraining height. The anchor side is a placement preference, not a height limit; vertical taskbars must not force a half-screen viewport.
- `availableHeight` is the full usable work-area height minus OS-required outer clearance, never only the space on one side of the anchor. `outerHeight = min(naturalContentHeight + nativeChrome, availableHeight)`. Up to 100% is allowed, not required. Short content stays short; fitting content has no primary scrollbar. No fixed 400/580/640 px or percentage runtime cap.
- `scrollViewport = outerHeight - nativeChrome - fixedHeader - fixedFooter`. All terms measured at actual font/DPI; clamp nonnegative. If chrome itself cannot fit, use a scrollable compact fallback retaining Back/Close and focused input, never an empty negative viewport.
- Coalesce layout passes; suppress repeated <=1 physical-pixel measurement jitter only when content remains fully reachable. A real 1-pixel overflow still requires fit/scroll handling. Do not resize continuously for same-size status updates.
- Open with correct size before exposure where supported. Changing monitor or taskbar placement recalculates usable space. Keep the anchor association while allowing vertical repositioning; do not cover the taskbar, menu bar or notch exclusion.
- Snapshot mode uses the same sizing algorithm as normal opening. Full-document diagnostic captures may exist, clearly labelled; they do not qualify visible-window fit.

## R-UI3 Scroll ownership

Apply [F3](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md#f3-scroll-and-focus). TaskQ surfaces:

| Surface | Fixed | Scroll only when needed |
|---|---|---|
| Home | view/admission/pause/Controls; Settings/AI footer | Work mode, attention, composer, aggregate, projects |
| Menu task list | window header row, version/resize footer | one shared scroll: host summary, global pins first, filters, task rows or Completed; no separately scrolling subpanels (P-W1) |
| Project / filtered tasks | Back, project/Board, current filter | task rows; all required states reachable |
| Settings detail | Back/title, contextual commit/cancel where present | settings groups, validation and detailed effects |
| AI conversation | window controls, context, composer | transcript; conversation list independent when side by side |
| Narrow AI | window controls and Back; composer in conversation view | full-width list OR conversation, not compressed columns |
| Change review | target context, Cancel/Confirm | long diff/evidence; buttons never cover last row |

- No-scroll means no reserved scrollbar strip unless native stable gutter is necessary to prevent wrapping oscillation; never use a visible fake track.
- Refresh keeps the focused task by identity. If it disappears, focus the nearest surviving item and announce the change.
- Test wheel/trackpad, thumb drag, PageUp/Down, Home/End, Tab and Shift+Tab. Last row and final action remain fully reachable.
- AI: growing input remeasures to its approved cap, then input scrolls. Closing a window preserves request identity.

## R-UI4 Cadence data

- Follow [approved decisions](../reviews/visual-decisions.md) and [I1–I6](icon-summary.md). Current task counters remain separate from event counts.
- All four surfaces share the selected period (default 24 hours). Exactly six equal source bins for every period, oldest left; presets use epoch-aligned whole bins of period / 6, newest containing now. Exact custom [start,end) bounds stay fixed.
- Deduplicate by board, event identity and kind before binning. Preserve closed, failed, ask and blocked counts and missing-source information.
- Closed alone determines height: 9 pt × closed / peak at the 22 pt reference. Peak includes only bins with complete source coverage, including observed counts in an ongoing bin. Peak zero is safe.
- Incomplete coverage never becomes zero or enters the peak. Detail retains observed lower bounds and missing sources. Collection start alone cannot prove uninterrupted coverage.

<a id="r-ui5-cadence-raster"></a>

## R-UI5 Canonical SVG

- One Rust function supplies Summary and its chart group to menu bar, running Dock, picker and History; no per-surface reconstruction, rebinning, heat opacity or stacks.
- Reference geometry, palette A and state marks are [I1–I4](icon-summary.md). Rasterize once at the native target size/backing scale; snap shared straight fill boundaries, never resize an intermediate bitmap.
- Picker embeds the vector at 18 CSS px. History projects the same chart group (reference x × 16, y × 8 at the 320 px plot), omitting current counters; responsive sizing never changes bins or normalized heights.
- History exposes six keyboard/click-selectable bins. Keep selection by [start,end) across refresh; it changes no scope, period, admission or task state. Detail shows interval, timezone, all four exact or explicitly incomplete counts, ongoing time and missing coverage. Legend totals carry completeness qualifiers.
- Native font/DPR, zero/unknown/ongoing/failed cues, pause and overflow need bounded owner review under R12. Source checks are not native acceptance.

## R-UI6 Feature parity and improvements

- Use the complete ledger in [acceptance](ui-acceptance.md); link implementation evidence per row.
- Preserve 4.1.9 Resources access, incident state/consent controls, capacity validation/readback, agent probes, focus recovery and Back position.
- Resources belongs under Controls or Services, not another permanent home block. Keep the existing external resource monitor until replacement is approved.
- Native system contrast, keyboard behavior or a useful implementation addition is an [F1](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md#f1-source-and-change-control) candidate.
- No missing feature is “polish”. Wrong scope, series, confirmation or session routing blocks acceptance before cosmetic review.
