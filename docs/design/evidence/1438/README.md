# Tray counter filters

Browser evidence for #1438 A2–A6 and the owner's open-count addition to A5.
R6 changes before implementation; K-A2/K-A6, UI25–UI28 and Menu M3 carry the decision (R13).

Before: base `377a424`, actual tray bundle, invented Alpha/Beta/Empty projects.
The new browser check failed K-A2: only Doing, Review, Ready and Closed appeared;
Ask, Failed, Blocked and Later were absent at zero. The two `before-*` images
show the separate checkbox group. Existing counter clicks replaced selection.

After: the same browser check passes all eight combinations of light/dark,
16/20 px and 320/360 px. `observations.json` records browser version, state
order, pressed states, visible outlines, width checks and six matching tasks
across Alpha/Beta. Every combination has `*-toggles.png` and `*-all.png`.
The images were visually inspected: capsules wrap in order without horizontal
overflow; All tasks stays in the same wrapping strip; project counters and
full-width task titles remain readable. Vertical scrolling at large type is expected.

The retained instrument checks OR, clicking off the last state, selectable zero,
unknown Closed, Reset, keyboard Enter/Space, focus retention, snapshot refresh,
Back, project/global scope and absence of queue/focus writes. Its old checkbox
assertions are superseded by R6/UI25–UI28, not removed risk protection.
`tests/project-blocks.browser.cjs` checks project identity and pressed Review
independently instead of depending on the old combined Filter wording.
`tests/project-blocks.test.cjs` verifies that All tasks deduplicates registrations,
counts held/open rows, excludes closed rows and Closed totals, and preserves unknown.

Reproduce from the checkout after the documented Playwright setup:

```sh
npm run build
FILTER_EVIDENCE=docs/design/evidence/1438 node tests/tray-state-filter.browser.cjs
node tests/project-blocks.test.cjs
node tests/project-blocks.browser.cjs
```

This is Chromium rendering of the actual tray bundle with a simulated Windows
snapshot and intercepted IPC. It is not live-board, native WebView2/WKWebView,
screen-reader or deployment qualification (R12). Source identity is recorded
in `sources.json`; the containing PR identifies the candidate commit.
