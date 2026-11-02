# Task execution health candidate

Candidate for [#2173](https://github.com/taskqorg/taskq/issues/2173), implementing its A1–A6 request. R6 and M3.13 amendments require explicit owner authorization under [the rule gate](../../rule-ids.md#contract-detail); the task currently names neither ID. This is a checkpoint, not implementation acceptance or a published result.

## Audit and attribution

The baseline is `df078e69872d7b936d6dfd9d829d7553f1525fc5`. R6 and §3 already separate ownership from execution, but `snapshot::stuck` excludes leased doing/review attempts from the two-hour inactivity rule. `profile watch` independently uses a one-hour board-comment gap. Neither boundary observes a pending tool's completion or deadline.

The worker log for #2157, attempt `6564771773`, native conversation `8faee200-bb7c-4d7e-80e5-066fa00ac4a1`, contains a `step_update` for `run_command`, step 90, with ACTIVE then DONE and `duration_seconds: 6620.665738`. The start's parameters contain `CommandLine` and no timeout. The owner PM readback at `2026-11-02T09:51:51.471996+00:00` attributes the stall to an isolated diagnostic Python `stdout.readline` without a deadline. It records termination of only the diagnostic leaf, preserving the service, real App writer and worker. The investigation read those records; it did not replay the diagnostic or terminate any process. Tool arguments and local paths are omitted here.

A fresh installed `taskq profile watch --since 1h` read succeeded: 8995 board calls, 0 failures, lowest remaining 2772. TaskQ cadence was closed 3, new 1, net -2, open 3. The baseline report listed no stall for #2157. These are observations of the existing installation, not proof of the candidate's deployment.

## Proposed behavior

The root R6 decision was edited first. The linked health contract defines independent host/attempt observation and native progress; bounded incremental reads; explicit unknown/error/stale handling; an advisory 15-minute no-deadline tool window; declared tool timeouts plus two polls; quiet reasoning without a pending tool; recovery clearing; and no replay, restart, release or admission authority. M3.13 needs a narrow exception for the requested local health diagnostic.

The candidate reaches runtime sources, the shared board/service snapshot, host execution heartbeat rows, CLI status, PM telemetry watch, Home/project/saved View rows, inline task details and Conversations Task info. Published workspace inputs and snapshot fixtures/schema are updated. It adds no dependency, collector, local task database or background agent nudge.

## Verification checkpoint

- Before: `heartbeat_health_is_explicit_even_without_an_execution_row` failed with `Null` instead of `unknown` on the baseline. The initial build also required the documented npm dependency installation; that setup failure is not the defect reproduction.
- After: the same test passed. All 19 snapshot checks passed, including existing execution and pending-input protections.
- Three native-event reader checks passed: the 6620-second unbounded tool scenario, concurrent tools, quiet work, explicit timeouts, reads without progress, partial records and replacement. The updated source additionally treats Claude Bash timeout values as milliseconds.
- `progress_health_lifecycle_captures_once_recovers_and_keeps_execution_fenced` passed: actual event-file reads through the service and board heartbeat to a remote snapshot, durable operation capture, one overdue/recovered event, published-notice restoration after service restart, unknown reads without false recovery, and unchanged attempt/execution ownership. The fixture panics on replay or termination.
- The PM watch threshold test passed. Existing cadence behavior remains covered by that check.
- `node tests/project-blocks.test.cjs`, `node tests/task-workspace.test.cjs` and `node tests/task-dashboard.browser.cjs` passed. The browser sequence checks accessible warning, unchanged row geometry, inline source/time/freshness/reason, and recovery clearing; existing narrow/large-text and navigation protections remain.
- Independent read-only source review found missing not-applicable progress, a diagnostic available only as an icon title, and volatile restart dedup. The candidate now uses not-applicable progress, a text tooltip on keyboard focus/hover and restoration from the matching published heartbeat. Final review of those corrections remains pending.
- `python3 scripts/check.py --rules` refused: `the task does not name M3.13 (changed), R6 (changed)`. The gate was not bypassed or changed.

## Remaining work

Inbox note `6574454403` adds the read-only desiign #14 case: logical Doing after an answer, with no new executing/accepted receipt. The candidate already leaves missing execution progress unknown, but an explicit awaiting-execution versus confirmed-executor diagnostic and its regression case remain pending. No action was taken on that task.

Native WKWebView task list/details/Conversations checks, the final independent readback, the selected canonical gate, the one pre-push rebase, PR publication and merge-result CI are pending. The fixture for bounded native captures is prepared, but no native capture has run. No service, session, receipt, grant, capacity or live policy was changed.

Restart dedup restores only an already published observation. A crash before the advisory heartbeat is published can leave capture/notification delivery uncertain; this is not exact-once crash-proof delivery. A source without native event timestamps uses append mtime as an explicitly labelled lower bound. Unsupported or invalid progress remains unknown.
