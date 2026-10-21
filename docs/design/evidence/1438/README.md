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

## Local checks and limits

- `python3 scripts/check.py --base c1007d6`: 83.438 s on this Windows host with
  a warm build cache; full Rust run: 427 passed, 21 failed, 36 ignored.
  Nineteen host-control/security checks are `sandbox-blocked` by worker authority
  guards or assert outcomes hidden by those guards. The Windows pipe deadline
  test and service publication-polling test also failed; their causes remain
  unknown. This is not a passing full gate. No worker authority was removed.
- R8's contract bound failed before shortening R6's link to its canonical detail;
  `core::tests::every_table_row_has_a_golden_trace` passes after that repair.
- All 20 `tests/*.test.cjs` component checks pass. The finding-composer provenance
  assertion first caught stale shared-component and source-manifest hashes;
  refreshing those two hashes repairs it without weakening its assertions.
- `cargo +stable run --locked -- selftest` in an isolated installation with
  `TASKQ_SELFTEST_OFFLINE=1`: 92 fold traces and format checks pass; board unknown.
- The three browser instruments above pass after rebase; the Home instrument
  additionally passes whole-line block alignment at 16/20 px.
- `python3 -m unittest discover -s tests`: no tests ran; no legacy qualification.

The README images were refreshed separately by Windows WebView2 snapshots from
source `59b321a6343439e9b442bc1eb3ef2bb071abc57d`, using `docs/shots/fixture.sh`
with Windows paths and freshly touched fixture status files, plus the existing
workspace fixture. `cargo +stable run --locked -- tray --snapshot DIR --route
project,icon` and the separate `workspace` route both exit successfully; the
manifest records the dev artifact, native renderer, source blobs and image hashes.
`sh docs/shots/check.sh` passes. The first stale fixture capture was discarded.
These images were inspected; owner visual acceptance and merge-group CI remain
pending. No installed service, live board, native filter interaction or release
deployment is qualified by these captures (R12, § 7.4).
