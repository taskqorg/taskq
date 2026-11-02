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

The canonical candidate gate and immutable independent review are recorded at the next checkpoint. They are not implied by these development checks.

## Review set and native handoff

[Native draft before](before-draft-idle.png), [native worker before](before-worker.png), [bundle idle](bundle-idle.png), [bundle unknown delivery](bundle-unknown.png), [bundle author/time](time/chromium-owner.png), [exact focus time](time/chromium-exact-focus.png). Native images use the installed app's existing window/text settings; a numerical font size was not independently measured. Bundle images are 940×740 / 20px instruments and were opened for visual inspection. They are not native-after evidence.

Root owns production GUI navigation under [6572406330](https://github.com/taskqorg/taskq/issues/2168#issuecomment-6572406330). Worker navigation stopped on reading that handoff. Worker saved draft `1c5aae2b624c462c0e674a151fe23ce7` and left it unsent; no Send, native session launch, production install, service restart, grant/capacity/model change or input replay occurred. Existing manager drafts, watches and unknown delivery operations were not edited. The selected draft not being visible in the old sidebar page is a recorded discovery/density gap, without a broader redesign here.

Root must install/reopen the exact candidate through the normal immutable dev handover and repeat real manager/worker loading, switching, question, reply and result journeys at actual text size. Compare polling/refresh/stall/failure transitions with the native before; preserve draft/files/focus/range, selected source/anchor and original receipts, without replay. A1 native-after, A2 native state matrix, A3 native placement and A4 real preservation remain Not tested on this candidate; A5 has independent source review but native comparison remains pending.

Prior 100ms interaction and 32ms frame failures, long-heap plateau unknown, Windows, remote/provider variants and screen-reader boundaries remain explicit. This correction does not claim a performance improvement or qualify those platforms. Merge-result `merge_group` CI remains required before publication (R12/section 7); the worker does not wait for it or install production.
