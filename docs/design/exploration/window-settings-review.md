# Window settings: exploratory review

Source: [window-settings.html](window-settings.html). SHA256: `e205ec8fe7d6904e01fc527ca2f0ac9ce0f86df7cef020de2569ffab6b2995f3`.

Scope: settings for existing growth behavior, following M13 and MP13. Current correction: owner requested immediate animation and an integrated corner grip. Prior route evidence below is retained from the preceding revision; changed visuals were inspected again. Reuses the selected menu, direct header navigation and filter editor. No installed preferences or runtime code changed. The owner correction replaces both OS-specific and universal Down defaults with actual opening-side input. Exact setting labels, miniature styling and section grouping remain exploratory.

| Rule / scenario | Observation | Result |
|---|---|---|
| M13 / placement | Auto miniature resolves up then down when the review input changes; no OS lookup | Pass for simulated vertical placement |
| M13 / override | Down remains selected while opening changes down/up. Pin, Close and reopen retain Down | Pass in browser |
| MP13 / direct choice | Four labeled choices visible together at470px, without dropdown or Apply. At320px/light20, two columns; client/scroll widths both318px | Pass for inspected fixtures |
| D3 / density and legibility | Dark16/470px and light20/320px screenshots inspected; icon, retained edge and final frame visible. Labels readable, no horizontal overflow | Pass for inspected fixtures |
| M13 / fixed chrome | Keyboard shrinking reaches470×260. Content viewport72px with243px content; End reaches scroll170.5 of171. Back and footer remain within the panel | Pass for exercised browser path |
| Persistence | Reload restores Down, Pin and470×260 dimensions | Pass for local preview |
| Resize completeness | Shrinking exercised. Increasing after reload timed out despite a visible grip; no console errors observed. Pointer drag not exercised; keyboard action timed out again in this revision | Not qualified |
| Autoplay correction | Growth/scroll animation names active with no hovered option; infinite4s cycles start on display. Static geometry and small intermediate changes observed. Reduced-motion guard retained | Autoplay state verified; full motion cycle not visually qualified |
| Corner grip / D11 | Dark470px and light320px/20px screenshots: diagonal strokes touch the lower-right corner without square fill/border.32px target sits1px inside both edges; accessible Resize window name retained. No horizontal overflow | Pass for inspected fixtures |
| Version uncertainty | Footer displays `TaskQ · v—`, accessible unknown-version label; no fixture version presented as installed | Pass |
| Syntax / model | Node syntax check; source-derived Auto, explicit-override, dimension-validation and clamp assertions; display/source equality | Pass |
| Native placement | Real icon coordinates, side-mounted taskbar, monitor/DPI changes, edge fallback, drag/pin geometry and native resize | Not tested |

The two review opening inputs are fixtures, not product settings or a native placement implementation. The footer is fixed outside content scrolling; dimensions appear after manual resize. The displayed version remains unknown. Main panel growth itself is not simulated by selecting a miniature: the setting and its preview change.

Corrections before presentation: removed OS/default-Down assumptions; added actual settings page instead of another behavior comparison; moved Back and footer outside the scroll viewport. Growth settings and saved manual dimensions still need native engineering qualification. The checkout's earlier implementation is not evidence against the owner's report about the installed app.

No cargo/runtime selftest, native qualification, board mutation, commit, push or deployment. Existing task-filter behavior is reused; unchanged journeys are covered by the earlier task-selection review, not requalified here.
