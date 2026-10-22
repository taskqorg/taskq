# Pinned-window growth: exploratory review

Source: [window-growth.html](window-growth.html). SHA256: `1c3c41989c9622d772cc5bb604a7b9b3dace7357bee5a96cd09eef5b62a27ab2`.

Scope: compare three owner-requested geometry choices through visible options, under MP13. Top-fixed Down, bottom-fixed Up and Fixed use the same list, font and initial frame. Four tasks are shown by default to expose the difference without an extra interaction. No product choice is inferred; M13/native sizing has not been amended to introduce a new growth mode.

This is an isolated behavioral diagram using simplified menu chrome and read-only task rows. It preserves metadata-before-title, All totals, pinned/close glyphs, consistent icon size and fixed chrome outside the scroller. It does not remove approved navigation or implement those static glyphs as working controls. It is not the full menu reference.

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow / meaningful comparison | All three visible simultaneously at desktop width, with labels identifying the edge retained. Top 2/4/8 controls update the same tasks in every preview; no bottom carousel | Pass |
| MP13 visible choice | Three labeled native buttons select a mode directly; no dropdown or extra Apply step. Initially none selected | Pass |
| Down invariant | Dark 16 px, 2→8 tasks: top remains124.203125; height231.59375→343.796875 | Pass in browser |
| Up invariant | Same route: bottom remains355.796875; top124.203125→12 | Pass in browser |
| Fixed invariant | Same route: top124.203125 and height231.59375 remain unchanged | Pass in browser |
| Overflow / R-UI3 direction | With two tasks all scrollers fit. With eight, content height537 exceeds viewports272/160; only bodies scroll. End in the fixed body reaches #113 at scrollTop377.5, with last task inside body bounds | Pass for browser diagram |
| D3/D6/D7 | Dark16 and light20 screenshots inspected. Full-width titles wrap under metadata; Ready uses neutral color, Doing/Review use work color; meaning has symbols and labels | Pass for inspected fixtures |
| Narrow fixture | Root320/light20 stacks the three previews. Window304 wide, no horizontal overflow; all frames stay within the 480 px demonstration stage. It is a readable fallback, not the preferred simultaneous review surface | Pass for inspected fixture |
| Selection / restoration | Selecting Fixed marks its button; browser reload restores Fixed and eight tasks. No installed preference is touched | Pass for local preview |
| Syntax/model | Script syntax, source-derived edge/bounds tests and inspected browser console pass | Pass |
| Native acceptance | OS window resize, monitor/DPI changes, work-area fallback, native Snap, real move/blur and preservation under live refresh | Not tested |

Correction before presentation: measured baseline includes consistent row dividers across all task counts; otherwise adding tasks shifted the supposedly retained edge by half a pixel. Rechecked the exact rendered source above after the correction.

The 480 px stage, baseline derived from two rows and 12 px clearance are explanatory dimensions. They do not replace R-UI2, choose a production fixed size or approve a monitor-edge fallback. Scenario changes deliberately return scroll to the beginning; live refresh must follow existing context-preservation rules. Header glyphs are diagram content, not inert product buttons.

No runtime/service, installed settings, task board, commit, push or deployment changes. Cargo/runtime selftest were not run for this prototype iteration. Mode default, fixed-size editing, per-display policy and full settings integration await design decisions.
