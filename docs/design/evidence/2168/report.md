# Conversations continuous stability

Implements [#2168](https://github.com/taskqorg/taskq/issues/2168). Baseline: `26b9e6615f75aab6577c1663fdec5d8c77335bfa`. The issue and owner steers retain the request verbatim. Applied R12 for before/after and platform boundaries, R13 and C2/D4 for source/draft preservation, and section 7 for the isolated branch and publication. D4 feedback records the accepted owner clarification before its implementation; no numbered rule, permission, runtime authority or admission policy changes.

## Failure before

- On the actual baseline bundle, a long unknown-delivery receipt shrank the reader from 560 to 488 CSS px and moved the composer from y560 to y464 at 940×740 / 20px. The footer grew with its content and the composer reason gained a line. `conversations-stable.browser.cjs` reproduces that failure before and checks the same geometry after.
- Native baseline `4.6.59-dev.1793597306` shows tool times at the right edge while ordinary author/time metadata stays at the left. A CSS-only counterfactual using the baseline stylesheet fails the new settled author/time alignment check; this is an instrument, not a second native run.
- Owner steer [6572650799](https://github.com/taskqorg/taskq/issues/2168#issuecomment-6572650799) supplies the [native duplicate stale notice](https://github.com/taskqorg/taskq/raw/1cdc6406da4b4511d7517b90899178b119ab969c/2168/b9e1af984e7b-owner-stale-notice-duplicate.png): the sidebar and composer show the same catalog reason. The owner reports repetition every few seconds; installed cadence attribution remains pending Root's candidate trace.
- The catalog poller assigned the scan's launch Instant to its completed result. The new Rust regression fails before: a newly completed read with an eight-second launch/poll delay immediately reports `complete:false` and the stale warning. It also aged genuine failures into the generic refresh reason.

## Candidate correction

Feedback occupies one fixed row using the existing 32/44px control target. Long text retains its full source in the accessible message and hover/focus tooltip. Existing recovery operation IDs and labels stay on directly reachable catalog-refresh, receipt-check, source-open and Undo icons. Ordinary refresh adds no warning; unchanged polling leaves the live feedback text untouched. Genuine workspace/save/AI errors retain alert semantics, including simultaneous errors. The reserved composer reason row keeps full keyboard-accessible text and its existing input description.

Catalog source age starts at actual read completion, carried through the asynchronous channel; receipt time does not renew it. The existing two-second refresh and four-second stale bound remain. Initial loading is quiet only while bounded: a stalled first read becomes explicit after four seconds. A failed/disconnected read retains its cause; stale conversion never overwrites it. Incomplete, stale and pre-mutation results cannot prove absence. UI Send still uses the unchanged `problem()` source fence. The sole catalog diagnosis is the fixed feedback row, with its original source time and explicit Refresh; sidebar/composer duplicates are removed.

The existing provider assets identify the recorded worker and composer runtime; unknown runtime IDs retain a neutral terminal fallback. Author metadata puts its compact time at the right edge, using the existing exact local date/zone/source tooltip. Transcript storage, native parsers, cursor bounds, source generations, drafts/files and immutable receipt handling remain unchanged.

## Verification

Selected source/component checks pass: relative-time formatting and PM composer boundaries; catalog polling completion/stale/error/epoch guards; fixed geometry, visible recovery and input hit testing at 940/350px; simultaneous alerts; quiet refresh, bounded unknown Send, one stale diagnosis, source time and unchanged-announcement checks. The Rust polling regression owns its cache and receives each real fixture reader's completion before teardown, rather than mutating the process-global catalog.

Actual-bundle Chromium/WebKit timestamp instruments pass 350/620/940px × 14/18/20px × light/dark, including exact keyboard tooltip, source attribution, minute-node identity, focus and reading anchor. Separate recovery instruments pass 16 Codex/Claude × theme × 18/20px × narrow/wide cells, with failed renderer, delayed save/read, retained files/caret/focus, exact pending operations and no replay. Worker streaming and PM-to-question-to-result instruments pass in their existing fixture scope. Native IPC and provider activity in these browser checks are substituted.

The timestamp fixture now targets the existing setup-evidence disclosure; its protection still requires exactly one recorded brief time. No source-time, no-replay or preservation assertion was weakened.

The canonical gate outcome and immutable independent review follow below. They are not implied by these development checks.

## Review set and native handoff

[Native draft before](before-draft-idle.png), [native worker before](before-worker.png), [bundle idle](bundle-idle.png), [bundle unknown delivery](bundle-unknown.png), [bundle author/time](time/chromium-owner.png), [exact focus time](time/chromium-exact-focus.png). Native images use the installed app's existing window/text settings; a numerical font size was not independently measured. Bundle images are 940×740 / 20px instruments and were opened for visual inspection. They are not native-after evidence.

Root owns production GUI navigation under [6572406330](https://github.com/taskqorg/taskq/issues/2168#issuecomment-6572406330). Worker navigation stopped on reading that handoff. Worker saved draft `1c5aae2b624c462c0e674a151fe23ce7` and left it unsent; no Send, native session launch, production install, service restart, grant/capacity/model change or input replay occurred. Existing manager drafts, watches and unknown delivery operations were not edited. The selected draft not being visible in the old sidebar page is a recorded discovery/density gap, without a broader redesign here.

Root must install/reopen the exact candidate through the normal immutable dev handover and repeat real manager/worker loading, switching, question, reply and result journeys at actual text size. Compare polling/refresh/stall/failure transitions with the native before; preserve draft/files/focus/range, selected source/anchor and original receipts, without replay. A1 native-after, A2 native state matrix, A3 native placement and A4 real preservation remain Not tested on this candidate; A5 has independent source review but native comparison remains pending.

Prior 100ms interaction and 32ms frame failures, long-heap plateau unknown, Windows, remote/provider variants and screen-reader boundaries remain explicit. This correction does not claim a performance improvement or qualify those platforms. Merge-result `merge_group` CI remains required before publication (R12/section 7); the worker does not wait for it or install production.

## Immutable source review

A second independent read-only reviewer checked exact `78d68469dd78ea4b7bfa055d85b2970ff784fcc7` against the baseline and found no P0/P1/P2 source blocker. Runtime/test source is `86ac61d7088716282c8f5f5e1e98ed3c050923c2`; the later commit adds evidence only. Review covered connected owner/task/result/error paths, tooltip/focus, visible feedback recovery, simultaneous alerts, completed-read freshness, bounded initial loading, failure/epoch/absence safety and the locally owned polling regression. It ran no native GUI or tests and does not establish native acceptance.

Final image regeneration visually confirms readable feedback/recovery, a clear composer, provider mark and exact local time. The full feedback remains text, including object-shaped native error reasons; unknown error shapes explicitly say details are unavailable. The rejected intermediate overlay design is not in this candidate.

## Final check checkpoint

After the single pre-push rebase, `python3 scripts/check.py --base 26b9e6615f75aab6577c1663fdec5d8c77335bfa` ran on `78d68469dd78ea4b7bfa055d85b2970ff784fcc7`. It exited 1 after 325.828 seconds within the 900-second budget. Rust: 882 passed, 7 failed, 41 ignored. All seven failures occur in unchanged `runtime::protected_app_server` fixture setup at line 140 with `Operation not permitted` (EPERM); the Seatbelt policy-store check separately reports sandbox-blocked. These are not passes. Rule gate passed; contract lint remains unknown because the checker's pinned specq checkout is absent locally. CI supplies that checkout. Environment: macOS worker sandbox, warm dependencies/build cache, Node v22.18.0, Python 3.14.8, Cargo stable 1.98.1. Local timing does not qualify cold CI.

Because the gate stopped at Rust, its remaining commands ran separately with the checker's isolated temporary-install environment, taking 106.564 seconds. `cargo +stable test --locked --test integration`: 22 passed, 1 failed, 1 ignored; the unchanged `telemetry_process::resume_records_its_result_without_releasing_the_host_hold` fails with `sandbox-exec: sandbox_apply: Operation not permitted`. Offline `cargo +stable run --locked -- selftest`, the README shot check/update scripts and release tag/check scripts pass.

Of 37 Node commands, 36 initially passed. The sole failure was `sync-view.test.cjs`, which matched the removed `footer` array spelling rather than the new shared `footerText` status channel. The corrected connection assertion passes. The existing actual-bundle stability instrument now also requires the board's `Data from HH:MM` text to remain visible in the status row without moving the input; it passes. Stale thresholds, unknown-data text, same-row Retry, pending identity and no-overlay checks remain. This is a test-connection repair, not a change to accepted behavior or protection (§7 step 4). The full gate was not repeated for this test/evidence-only correction; unchanged source results above are retained with their sandbox limits.

The final source remains the independently reviewed `86ac61d7088716282c8f5f5e1e98ed3c050923c2`; subsequent changes contain evidence and the stale-feedback test repair only. Native acceptance remains pending Root's exact-source handoff; no result here asserts A1–A5 complete.
