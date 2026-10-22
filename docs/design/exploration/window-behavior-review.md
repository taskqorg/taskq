# Window behavior: exploratory review

2026-10-22. Source: [window-behavior.html](window-behavior.html). SHA256: `702cd95c3a9e63be8ba4bf8729f32cc9a40776fcf40909fef5c1b59a442ee8cb`.

Owner selection: direct header navigation under MP12. Window targets amended under M13: always-accessible Close, unpinned header drag enables Pin in the same gesture, last mode retained. Product pointer updated first in taskq.md under R13; this is design work, not a runtime implementation.

Baseline: `src/tray.html` starts header dragging only when `S.pinned`; `src/tray.rs` accepts drag only while pinned. Existing source persists Pin and per-display position and hides an unpinned window on blur. That source was read, not changed or qualified live.

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow / MP12 | One selected direct-navigation window; consolidated alternative removed from the visual. Existing filter overlay and task rows retained | Pass |
| D3/D6/D9, visual system | Dark 470 px / 16 px and light 320 px / 20 px inspected in home and Settings. Narrow app scrollWidth=clientWidth=318. Seven icon actions measure 32 px wide; host handle 64 px. Auto text moves to accessible host details at narrow widths | Pass for browser fixtures |
| M13 Close | Shared header Close works on home and Settings; reopening Settings retains that section. Close hides a manually pinned panel without clearing Pin | Pass for local prototype |
| M13 working context | TaskQ preset + Recover search show #103. Pin → Close → reopen retains that view and Pin; browser reload also retains Pin and query | Pass for exercised local path |
| M13 outside interaction | Click on preview background closes unpinned panel; the same outside interaction leaves pinned panel open | Pass for local outside-click model; native blur untested |
| D8 / M13 click versus drag | Ordinary host-button click opens host details while Pin stays off. Model assertions cover movement threshold, first displacement, bounds and saved-position validation | Pass for click/model only |
| M13 actual drag-to-pin | Browser drag command produced no observed displacement or Pin transition. No lower-level injection used to substitute evidence | Not tested successfully; real gesture remains unqualified |
| M13 Escape | First Escape closes host popover; next hides panel. Reopen retains filter/search and mode | Pass for local path |
| M13 native persistence/placement | OS always-on-top, fixed header during native scrolling, focus loss, tray click, per-display position, off-screen fallback, Windows Snap, macOS clamp | Not tested |
| Runtime / source checks | Script syntax, fragment shape, selected layout, display-copy equality and root-contract size checks pass; inspected browser route reports no console errors | Pass for inspected source/route |

Movement is limited to the conversation preview stage. Its header movement handler and model assertions are not native-window proof. Preview review buttons are outside product chrome. The full product scroll viewport is intentionally not reproduced inside this content-sized conversation preview.

No service, task, permission, installed app or native runtime was changed. No commit, push or deployment. Cargo/runtime selftest were not run for this documentation/prototype iteration. Settings taxonomy, counter semantics, task total-age placement and full conversation workspace remain open.
