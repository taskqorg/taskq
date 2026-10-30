# #1957 macOS native evidence for #1940 A7

This is native macOS before/after evidence for [#1940](https://github.com/taskqorg/taskq/issues/1940). It covers the [PR #1956](https://github.com/taskqorg/taskq/pull/1956) candidate and the Windows package `docs/design/evidence/1940/README.md` on that branch. Evidence only: the implementation is unchanged and PR #1956 is not merged.

## Identity (A1)

| Item | Value |
|---|---|
| Candidate | PR #1956 head `1e9912569f9b7374ae119bd8da0534064619b3d2` (open), unmodified `git archive` |
| Baseline | `be3c20f3337d8db3e2c7bb555bbae777247c2bb2` plus [harness-baseline.diff](harness-baseline.diff) |
| Walk build | candidate plus [harness-walk.diff](harness-walk.diff) |
| Host | macOS 27.0.1 (26A434), arm64, WKWebView (WebKit 22625.1.29.11.28) |
| Build | `cargo build --release --locked`, then signed by `scripts/macos-runner.sh` (Authority "TaskQ Local Signing") |
| Install | temporary `TASKQ_INSTALL` from `docs/shots/fixture.sh`, with no board token. The production service, grants and owner sessions are untouched. |
| Fixtures | invented. Candidate: `tests/fixtures/reports-shots.js` (#1820 report, #1821 task). Baseline: [results-before-1957.js](results-before-1957.js) (same #1821). Walk: [results-walk-1957.js](results-walk-1957.js). |

The binary SHA-256 values, fixture hashes, routes, exit codes and elapsed times are in [native.json](native.json).

The baseline has no `results` route, so the harness adds two snapshot routes and changes no UI code. The routes feed the same invented #1821 through the baseline Completed history read (`{a:'completed'}`). The baseline `reports` routes are unchanged.

The walk build adds two `walk` routes and raises the capture delay from 800 ms to 4 s, so that the scripted walk finishes before the capture.

## Review set (A2): 10 screens, dark, 16 px

| Surface | Before (be3c20f3) | After (#1956) |
|---|---|---|
| Menu, report | [before-reports.png](before-reports.png): "Reports · 1" sits below the "Tasks unknown" dashboard, and the report runs below the fold | [after-reports.png](after-reports.png): "Results · 1" opens first with Previous/Next and Make tasks/Later/Drop |
| Conversations, report | [before-reports-conversations.png](before-reports-conversations.png) | [after-reports-conversations.png](after-reports-conversations.png) |
| Menu, completed task | [before-results.png](before-results.png): a Completed history row only ("Page 1 loaded"), with no result body and no action | [after-results.png](after-results.png): attention badge 1, "Results · 2 · 1 new", Changed/Checks/Open, Accept/Follow-up task/Reopen |
| Conversations, completed task | [before-results-conversations.png](before-results-conversations.png): no completed work, "No threads yet" | [after-results-conversations.png](after-results-conversations.png): the same result card with its actions |
| Native walk | [walk-menu.png](walk-menu.png): 16/16 PASS | [walk-conversations.png](walk-conversations.png): 15/15 PASS |

The light variants were captured as well (exit 0) and were not committed. They are listed in `native.json`.

## Walk (A3)

The walk script runs inside WKWebView on the candidate. It mirrors `tests/results.browser.cjs` and `tests/reports.browser.cjs`. All receipts are invented and answered inside the page, so no decision and no draft reaches a board. Steps:

- Cached list: the unread badge "2 new" (menu) or a "New result" row (Conversations), then two rows marked Unread.
- Navigation: open #13, Next to the report, Previous back.
- Completed task: Accept ("Result accepted."), Reopen with its reason kept ("Read held draft #25"), Follow-up.
- Unavailable board: Reopen stays "Board unavailable; acknowledgement unknown" with the card kept.
- Seen state: Read, not Unread, after Back.
- Report: Later ("Kept in Reports for later."); Make while the board is unavailable keeps the card ("Board unavailable; decision unknown"); Make ("Created held drafts: #21"); Drop ("Report dropped.").
- Board reads: zero `list` and `open` requests.
- Unavailable collection: names "Research: Report collection is stale; waiting for the service".
- Menu only: stale Completed history keeps its rows and the "Collection stale" reason.

Walk time inside the page: menu 2067 ms, Conversations 1047 ms. These times include the 400 ms polling caps of the absent-button probes.

## Telemetry and unknown reasons (A4)

[measure.py](measure.py) runs the candidate release binary in an isolated root with no token. It produced [cached-reads.json](cached-reads.json).

| Read | Wall time (ms) |
|---|---|
| `reports --json` (9 rows) | 43 |
| `reports 1 --action open` | 41 |
| `results --json` (19 rows) | 27 |
| `results 101` / `results 102` | 43 / 25 |

The CoreQ operations `reports.list.cached` and `reports.open.cached` took 6–26 ms (13 events). The full list of recorded operations contains no collection, board or provider operation, and `status.json` stays byte-identical.

Each unknown case returns `complete:false` with an explicit reason, for both `reports` and `results`:

| Case | Reason |
|---|---|
| missing `status.json` | Reports not collected yet |
| `observed` 24 h old | Report collection is stale; waiting for the service |
| service error field | the error text itself (invented: "Board unavailable; collection unknown") |
| invalid collection | Report collection invalid; waiting for the service |

## Checks (A5)

| Check | Result |
|---|---|
| A1 candidate, baseline, hashes, macOS/WKWebView, fixture identity pinned | Pass |
| A2 native before/after for reports and completed tasks, Menu and Conversations, ≤10 screens | Pass (10) |
| A3 cached list/open/next/previous, seen state, Make/Later/Drop, Accept/Follow-up/Reopen | Pass (menu 16/16, Conversations 15/15) |
| A4 cached timings, zero board reads on open, explicit unknown reasons | Pass |
| TCC denial | None: every capture exited 0 without a privacy prompt |
| Real report decision or real Reopen/Follow-up against a board | Not tested (invented receipts only, by design) |
| Native menu-bar click path (status item to popover) | Not tested (`tray --snapshot` renders the same bundle in its own window) |

## Reproduce

```sh
git archive 1e9912569f9b7374ae119bd8da0534064619b3d2 | tar -x -C cand
cd cand && cargo build --release --locked
TASKQ_INSTALL=$tmp/install scripts/macos-runner.sh target/release/taskq tray --snapshot $out --route results
python3 docs/design/evidence/1957/measure.py target/release/taskq
```

For the baseline, use `be3c20f3` with `harness-baseline.diff` applied and `results-before-1957.js` copied to `tests/fixtures/`. For the walk, use the candidate with `harness-walk.diff` applied and `results-walk-1957.js` copied to `tests/fixtures/`.
