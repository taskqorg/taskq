# Mac Drain and Full stop: #1214

Owner-approved scope: both Stop modes with a durable host hold. Selective recovery, preserved-work selection and native-release gates are tracked by [#1285](https://github.com/taskqorg/taskq/issues/1285), which depends on #1214. R11/E2 now permit owner-confirmed local cancellation before board retirement, including offline. R1 still owns task truth on the board; this change uses its existing host-hold exception.

## Behavior and limits

Drain holds new admission while current work finishes. Full stop requests cancellation through retained native worker/reviewer/operator handles before board access. Existing exit and execution-release checks remain authoritative. Neither a successful cancellation call nor a stale snapshot proves termination. A current project observation must match the live host generation before the Stop page displays native progress.

The reviewed lifecycle request pins exact roots, host generation and command identity. Its hold survives daemon exit and restart, refuses ordinary hold/release replacement, and pins the launcher rollback floor. No implicit cleanup, reset, stash or workspace deletion is added. Recovery remains unavailable until #1285. Do not downgrade or manually remove the hold to resume. Reverting code cannot restore unsaved buffers or undo external effects already performed.

## Regression evidence

- `cargo test --locked full_stop -- --nocapture`: before the cancellation implementation, `full_stop_cancels_owned_work_before_board_access` failed: observed `None`, expected `Some(-9)`. After: all three Full stop checks passed. Drain leaves the owned worker running; Full stop cancels it with board comment writes failing, retains native ownership, preserves the original board attempt and keeps the hold.
- The lifecycle test preserves request scope/idempotency, refuses a changed mode under the same identity, and rejects corrupt holds without manufacturing an exit receipt. Its installation fixture gained `current`, required by rollback-floor validation; assertions were retained.
- The operator test requires cancellation before observation and keeps unknown/pending native reads unresolved until exit. The pre-existing unknown-observation test still forbids cancellation without Full stop.
- `node tests/tray-action.test.cjs`: both modes, exact submitted mode, unknown stale progress and retained hold pass alongside existing receipt/generation tests.
- `tests/tray-stop.browser.cjs`: bundled UI, dark 16px/320px and light 20px/360px, one Full stop IPC request, queued acknowledgement, unavailable cancellation, stale/foreign-generation refusal, no horizontal overflow or page errors. Browser rendering is not native process proof.
- Initial full gate: 385 passed, 30 ignored, one failure: contract exceeded 60 KiB (R8). R11 detail moved to linked E2 without weakening protection; contract is 61,437 bytes.

Native execution cancellation on installed agent clients, remote-host behavior and the three existing physical UI exceptions are not requalified by fake-process or browser checks. No production Stop, permission change, deployment or Windows operation was performed.

## Candidate checks and visual evidence

Candidate source: `eed78f6e` (same implementation as `da6d02e5`; contract-size repair only).

`python3 scripts/check.py --full --base origin/main` after the R8 repair: 386 unit checks and 10 other integration checks passed; the telemetry process suite passed six cases but its nested sandbox case failed with `sandbox-exec: sandbox_apply: Operation not permitted`. Outcome: **sandbox-blocked**, not PASS. The gate took 85.082s on this warm Mac build (initial failing run: 84.747s); no retry of the blocked case. Merge-result CI remains the acceptance gate under R12/§7.

The remaining gate commands ran separately: isolated `taskq selftest` passed 82 golden traces and formats (board deliberately unknown/offline); all `tests/*.test.cjs`, screenshot-manifest fixtures, release tag/check fixtures and the changed bundled browser instrument passed.

Native `tray --snapshot --route stop` used an isolated `docs/shots/fixture.sh` installation with no host service. Both themes and 16/20px were captured. This proves WKWebView rendering of the unavailable-service state, not a working production Stop. The disabled choices, named scope and unknown state are visible. At 20px the bounded native viewport requires scrolling; physical scrolling/keyboard acceptance is not claimed.

- [Native dark 16px](native-dark-16.png)
- [Native light 20px](native-light-20.png)
- [Browser reviewed scope](browser-review.png)
- [Browser unknown cancellation](browser-unknown.png)

The baseline had no dedicated Stop review route. Existing lifecycle receipt and native exit assertions were retained. No new physical UI exception is introduced; owner visual acceptance and installed-client qualification remain open. README snapshots are refreshed separately by the repository's existing `docs/shots/update.sh` and manifest protocol; refreshing images is not owner acceptance.

Final hold review found and reproduced a second defect: a new Stop could overwrite a malformed structured hold. The added assertion failed before the fix (`corrupt Stop identity cannot be overwritten`). Reusing the strict hold reader before persistence fixes it; `cargo test --locked --bin taskq lifecycle::tests -- --nocapture` then passed seven cases (one pre-existing ignored fixture). Identity, replay, original receipts and native-exit assertions remain intact. This later change affects hold validation only; the captured UI source is unchanged.

`sh docs/shots/update.sh` completed on source `2117dfd07d431119e1e811ad9cd1773ba53f85b3`; release build and native README captures succeeded. `sh docs/shots/check.sh` passed. The refreshed Attention and Projects captures were inspected; the Stop entry is visible with the existing content and footer retained. Manifest source/binary hashes identify that rendering candidate, not installed-client cancellation acceptance.

## Merge-conflict repair

Rebased onto `287e185a581333c7717fc8ca676a9795b53ab6a2` after the merge queue reported `merge_conflict`. The constructor now retains both Stop progress and the upstream runtime usage-limit map. Both regression groups and the upstream usage-limit retry rule are retained; no assertion was removed or weakened.

The combined contract failed R8 at 61,563 bytes. Wording-only reductions keep the existing R11/E2 and sections 2/3/5 decisions at 61,436 bytes; the unchanged size/golden-trace test then passed. The first gate also inherited worker context and correctly refused owner-only fixture operations. Test-only subprocesses subsequently cleared `TASKQ_ATTEMPT`, `TASKQ_TASK` and `COREQ_TRACE_CONTEXT`; production commands retain them.

The final `python3 scripts/check.py --base origin/main` passed 390 unit tests (30 ignored) and 10 integration tests, then reached the same nested `telemetry_process` sandbox refusal: six cases passed; `resume_records_its_result_without_releasing_the_host_hold` reported `sandbox_apply: Operation not permitted`. Outcome: sandbox-blocked, not a passing full gate; 43.335s on the warm Mac build. No native cancellation, deployment or Windows operation was performed. Merge-result CI remains required.

After that blocked gate, isolated offline `taskq selftest` passed 85 golden traces and formats (board unknown/offline); every `tests/*.test.cjs`, screenshot-manifest fixtures, release tag/check fixtures and `node tests/tray-stop.browser.cjs` passed. `sh docs/shots/check.sh` also passed. Existing visual evidence remains applicable to unchanged UI sources; owner acceptance and installed-client qualification remain open.

## Current-main conflict repair

Rebased onto `f32be16c` after the merge queue again reported `merge_conflict`. The conflict in `Service::new` now retains both upstream `project_revision` and Stop `stop_progress`. The upstream shared resource blocker projection is retained. No assertion, authority boundary or Stop behavior was removed or weakened; R11/E2 and R8 sections 2/3/5 remain unchanged. The contract is 61,429 bytes.

`python3 scripts/check.py --base origin/main` initially failed 16 owner-only fixtures under inherited worker context (110.453s). Isolated test subprocesses cleared only `TASKQ_TASK`, `TASKQ_ATTEMPT` and `COREQ_TRACE_CONTEXT`; production commands retain them. `cargo +stable test --locked` then passed 392 unit tests (30 ignored), 10 integration tests and six telemetry process cases. The nested sandbox case remains **sandbox-blocked** with `sandbox_apply: Operation not permitted`; the full gate is not PASS.

Isolated offline `target/debug/taskq selftest` passed 85 golden traces, formats and local telemetry capture; board state remains unknown/offline. Every `tests/*.test.cjs`, screenshot-manifest and release fixtures, and `node tests/tray-stop.browser.cjs` passed. Merge-result CI, owner visual acceptance and installed-client cancellation qualification remain open; no deployment or production Stop was performed.

`sh docs/shots/update.sh` refreshed the native README manifest on `fc893e10`; `sh docs/shots/check.sh` and `git diff --check` passed. Attention and Projects captures were visually inspected: the Stop entry, content and footer remain visible without clipping. This is rendering evidence only.
