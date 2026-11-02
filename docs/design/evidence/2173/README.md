# Task execution health candidate

Candidate for [#2173](https://github.com/taskqorg/taskq/issues/2173), implementing its A1–A6 request. Owner answer `6574730504` approves the R6 and M3.13 amendments under [the rule gate](../../rule-ids.md#contract-detail). This document separates source, model/process and native UI evidence; it does not claim a live installation.

## Audit and attribution

The baseline is `df078e69872d7b936d6dfd9d829d7553f1525fc5`. R6 and §3 already separate ownership from execution, but `snapshot::stuck` excludes leased doing/review attempts from the two-hour inactivity rule. `profile watch` independently uses a one-hour board-comment gap. Neither boundary observes a pending tool's completion or deadline.

The worker log for #2157, attempt `6564771773`, native conversation `8faee200-bb7c-4d7e-80e5-066fa00ac4a1`, contains a `step_update` for `run_command`, step 90, with ACTIVE then DONE and `duration_seconds: 6620.665738`. The start's parameters contain `CommandLine` and no timeout. The owner PM readback at `2026-11-02T09:51:51.471996+00:00` attributes the stall to an isolated diagnostic Python `stdout.readline` without a deadline. It records termination of only the diagnostic leaf, preserving the service, real App writer and worker. The investigation read those records; it did not replay the diagnostic or terminate any process. Tool arguments and local paths are omitted here.

A fresh installed `taskq profile watch --since 1h` read succeeded: 8995 board calls, 0 failures, lowest remaining 2772. TaskQ cadence was closed 3, new 1, net -2, open 3. The baseline report listed no stall for #2157. These are observations of the existing installation, not proof of the candidate's deployment.

## Approved behavior

The root R6 decision was edited first. The linked health contract defines independent host/attempt observation and native progress; bounded incremental reads; explicit unknown/error/stale handling; an advisory 15-minute no-deadline tool window; declared tool timeouts plus two polls; quiet reasoning without a pending tool; recovery clearing; and no replay, restart, release or admission authority. M3.13 adds the approved narrow exception for the requested local health diagnostic.

The candidate reaches runtime sources, the shared board/service snapshot, host execution heartbeat rows, CLI status, PM telemetry watch, Home/project/saved View rows, inline task details and Conversations Task info. Published workspace inputs and snapshot fixtures/schema are updated. It adds no dependency, collector, local task database or background agent nudge.

## Verification

- Before: `heartbeat_health_is_explicit_even_without_an_execution_row` failed with `Null` instead of `unknown` on the baseline. The initial build also required the documented npm dependency installation; that setup failure is not the defect reproduction.
- After: the same test passed. All 20 snapshot checks passed, including existing execution and pending-input protections.
- Three native-event reader checks passed: the 6620-second unbounded tool scenario, concurrent tools, quiet work, explicit timeouts, reads without progress, partial records and replacement. The updated source additionally treats Claude Bash timeout values as milliseconds.
- `progress_health_lifecycle_captures_once_recovers_and_keeps_execution_fenced` passed: actual event-file reads through the service and board heartbeat to a remote snapshot, durable operation capture, one overdue/recovered event, published-notice restoration after service restart, unknown reads without false recovery, and unchanged attempt/execution ownership. The fixture panics on replay or termination.
- A new cadence assertion failed before the fix: a 10-second quiet sample changed the board heartbeat time. After excluding only `progress.observed` from heartbeat change comparison, the same lifecycle check passed. Actual progress/status changes still publish immediately.
- The PM watch threshold test passed. Existing cadence behavior remains covered by that check.
- `node tests/project-blocks.test.cjs`, `node tests/task-workspace.test.cjs` and `node tests/task-dashboard.browser.cjs` passed. The browser sequence checks accessible warning, unchanged row geometry, inline source/time/freshness/reason, and recovery clearing; existing narrow/large-text and navigation protections remain.
- Independent read-only source review found missing not-applicable progress, a diagnostic available only as an icon title, and volatile restart dedup. The candidate now uses not-applicable progress, a text tooltip on keyboard focus/hover and restoration from the matching published heartbeat. The independent source checkpoint at `3d58fb2b` reran the lifecycle check successfully; the subsequent visual review found the list gaps recorded below.
- Before owner approval, `python3 scripts/check.py --rules` refused: `the task does not name M3.13 (changed), R6 (changed)`. After answer `6574730504`, the rule gate passed. The gate was not bypassed or changed.

## Accepted-input regression

Inbox note `6574454403` adds the read-only desiign #14 case: logical Doing after an answer, with no new executing/accepted receipt. Its regression failed before the fix with a missing executor state, then passed: the heartbeat now distinguishes board input awaiting execution/acceptance from an active exact executor and stale/unknown observation. No action was taken on that task.

## Native UI acceptance

The isolated capture runs the real signed macOS tray/WKWebView renderer with invented data from `tests/fixtures/execution-health.js`. It performs no board write or session operation. Candidate: `3d58fb2b` plus the warning-glyph and Conversations-row fixes committed with this evidence. Artifact SHA-256: `fefd0a96bcb2f684653d8d36ef1c31fa7f5047f90f039855fc1527fc667142b7`; fixture SHA-256: `c8adc331bbf52301988da77838aa721d5209ca91feb7f10f0b1a6e30b7365889`.

Platform: macOS 27.2 arm64, native WKWebView, existing app font, dark theme, 16px setting, native chrome. Capture sizes: list/recovery 640×1218, inline details 640×1844, Conversations 2200×1800. Monitor usable rectangle and OS accessibility interaction were not measured. Native snapshot mode preserves the owner's focus; keyboard diagnostic visibility is checked in the browser instrument.

| Acceptance | Action and expected behavior | Observed evidence |
|---|---|---|
| A2 | Open inline details: independent heartbeat source, observation/age, freshness, executor and progress reason | [Details](details.png): readable fields with wrapping and no clipped health content; exact timestamps use the existing focus/hover time control |
| A2 | Open Conversations Task info | [Conversations](conversations.png): same shared health fields; invented fixture has no native session target, so unavailable actions remain explicit |
| A3/A4 | Show overdue tool, healthy quiet work and input awaiting execution together | [List](list.png): local warning on overdue/awaiting rows; quiet row has none |
| A5 | Complete the pending tool and refresh the same list | [Recovered](recovered.png): tool warning clears; awaiting-execution warning remains; row height and width are unchanged |

Before visual correction, the native list had accessible warning attributes but no visible glyph: `circle-question-mark` was absent from the component's asset map. The added visible-glyph assertion failed before the fix. Reusing the bundled `triangle-alert` fixes the shared renderer; Node, browser SVG-bounds assertions and native capture then pass. Browser keyboard focus exposes the diagnostic text. No warning depends on color alone.

Final independent review found an additional A3 gap: Conversations has its own task list and lacked the shared warning. The browser reproduction failed with zero warning nodes instead of one before the fix. Thread projection now carries the exact task state/heartbeat; the sidebar reserves the same warning slot and exposes a textual diagnostic on row focus without nested controls. The same reproduction passes, including visible SVG, tooltip viewport bounds, recovery geometry and no execution-write messages. The existing Conversations grouping, receipts, recovery, drafts and focus browser check also passes. The native Conversations fixture asserts its accessible warning and nonzero glyph bounds. Final independent source and visual review of all four updated captures passed with no additional A2–A6, R6 or R12 gap.

The Conversations capture verifies Task info and the sidebar health warning. Its invented history has no executor rows or transcript, so the pre-existing activity panel says unknown; this is not a live executor qualification.

## Final checks and limits

- `python3 scripts/check.py`: not green. Rust: 896 passed, 7 failed, 42 ignored; total local gate elapsed 331.576s of 900s. All seven failures occur while protected app-server fixtures write `home/.codex/config.toml` at `src/runtime/protected_app_server.rs:140`, with `Operation not permitted`: **sandbox-blocked**, not PASS. The protections and assertions were not weakened.
- `cargo +stable test --locked --test integration`, with the checker's isolated child environment: 22 passed, 1 failed, 1 ignored. `telemetry_process::resume_records_its_result_without_releasing_the_host_hold` is **sandbox-blocked** at `sandbox-exec: sandbox_apply: Operation not permitted`.
- Offline staged `cargo run --locked --bin taskq -- selftest`, in an isolated temporary installation: PASS, 130 fold traces, format checks and 8 committed telemetry observations. Board/tools remain unknown because offline mode deliberately skips them.
- All 41 Node/artifact-script remainder checks passed, including the component, shot-tool and release-source protections. After the final Conversations correction, project-blocks, workspace, Conversations cache/overflow Node checks and the task-dashboard/Conversations-sidebar browser suites passed again.
- Native `cargo run --locked --bin taskq -- tray --snapshot <temporary-output> --route health-list,health-detail,health-recovered` and `--route health-conversation`: PASS in the isolated fixture installation, followed by visual inspection of the four distinct captures above.
- `python3 scripts/check.py --rules --base origin/main`: PASS for the entire branch after owner approval, changed IDs R6 and M3.13. Canonical contract lint remains unknown locally: its pinned specq `39e6024037fb` checkout is absent; the supplied rule cache contains Markdown only. CI must run that lint.
- The one pre-push rebase on `origin/main` completed before the canonical gate. No service, session, receipt, grant, capacity or live policy was changed. No action was taken on desiign #14.

PR and merge-result CI are separate publication evidence. This candidate is not installed; green `merge_group` remains required under R12 and §7. Native Linux/Windows execution and production event-source qualification are not claimed by these macOS fixture captures.

Restart dedup restores only an already published observation. A crash before the advisory heartbeat is published can leave capture/notification delivery uncertain; this is not exact-once crash-proof delivery. A source without native event timestamps uses append mtime as an explicitly labelled lower bound. Unsupported or invalid progress remains unknown.
