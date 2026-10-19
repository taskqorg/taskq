# Mac admission and telemetry repair

Candidate: v4.6.20, task #1276. The task result and PR head bind the exact worker SHA; the publication receipt binds the reviewed merge source. CoreQ remains pinned to `5fb10926bbca0fdb042342dc5dc6ac3cc4e5a56f`. No board format, grant, capacity, model, effort or host policy changes.

## Repair and evidence

- R12 and §3: inventory refresh starts at 30 seconds while the existing 60-second freshness bound remains. Starting a refresh does not replace the last completed observation with `pending` or renew its age. Stale, unknown and present native units still block admission; registered legacy ownership is checked on each pass.
- §3 and §4: a last-moment admission refusal before native submission retains the in-memory launch, reservation, execution intent and undelivered input. Worker, reviewer and resume retry that intent only while this process retains proof that native submission never happened. It writes no `gone` or `released` and consumes no task retry. Restart still treats unresolved native ownership as unknown.
- R12: mixed report/export takes the shared budget lock before the universal SQLite snapshot, matching writer lock order. Archive rows and the import cursor remain one coherent view. The guard is released before decoding/exporting universal rows. Existing 1-second lock and 3-second receipt/flush deadlines remain unchanged.
- §7.4: independent Resume and scheduler fixture processes discard the runner's inherited trace. Explicit continuation coverage remains in `independent_cli_preserves_the_dispatch_trace_on_failure`. The receipt fixture retains the full saved diagnostic and store timing on failure, reusing PR #1266's diagnostic change. Terminal selftest failures now include the actual error. No assertion, deadline, committed-receipt or native-exit requirement was weakened.

Before evidence in [evidence/1276](evidence/1276): the refresh-boundary test failed; the admission test consumed the worker attempt; the lock-order test entered SQLite while the budget lock was held. These are deterministic boundary reproductions, not reproductions of every historical incident.

Historical Windows CI jobs [117886122027](https://github.com/taskqorg/taskq/actions/runs/39198412756/job/117886122027) and [117889056092](https://github.com/taskqorg/taskq/actions/runs/39199391149/job/117889056092) were read. The first lacks complete receipt timing; the second reports `budget-locked` and a terminal selftest assertion. Their exact causes remain unproved on Mac. A green rerun alone cannot close those claims.

The unchanged installed v4.6.19 selftest passed 82 golden traces, formats, board access, terminal records and committed local capture during diagnosis. Thus the intermittent live lock timeout did not reproduce in this read. Cached service status reported the existing host PID and six projects; native process inspection was sandbox-blocked. CLI capture health is not resident-service health.

## Publication and operator handoff

The worker pushes one version PR and hands it to review without waiting for CI (§7). Publication requires green GitHub merge-result (`merge_group`) CI, or the configured private exact-SHA fallback. The version merge starts the normal release workflow (§8); a worker result does not claim a release or activation.

After publication, use the existing [Mac operator procedure](release-1233.md#bounded-outside-service-procedure), retaining the one service, six registered roots, open queue, dirty worktrees and current grants/settings. Do not repeat the first paid/native qualification turn or operate Windows/FRA.

Bind the downloaded `taskq-4.6.20-macos-aarch64` artifact and `SHA256SUMS` to the exact published tag/merge SHA and verify their provenance before activation. Record the actual artifact digest then; no artifact digest exists in this source-only handoff. Run the changed-path admission/selftest/receipt qualification and read the actual resident executable identity, all six fresh root publications and capture health. Keep native activation unknown until those observations pass.

Rollback uses the preserved v4.6.19 source `bf41d2f673951a6d19e81c39a8d72ad9cf9daf77` and the existing [rollback procedure](release-1233.md#failure-and-rollback). Verify the previous artifact and ownership first; never force-stop live children. Keep telemetry/archive files, worktrees and policy originals. Rollback cannot recover already lost observations or undo board transitions; it restores the old admission race too.

## Local verification

Development checks on macOS: the three boundary regressions passed after repair inside the parallel unit suite (383 passed, 30 ignored). Existing terminal-record, receipt isolation, native-exit and host barriers remain in that suite. The full selected gate reached 383 passing unit tests and passing host-policy, host-restart, resource-policy and service-log integration tests; telemetry processes had six passes and the protected-export case was sandbox-blocked (`sandbox_apply: Operation not permitted`). It is not an overall PASS. The unchanged seatbelt instrument also reported sandbox-blocked.

The worker test runner inherited task and trace context. Fixture-only test commands removed `TASKQ_ATTEMPT`, `TASKQ_TASK` and `COREQ_TRACE_CONTEXT`; board delivery commands retain the actual worker fence. This does not change any host grant or runtime setting. Tests used temporary installations while cached status reported the existing service running. Native service identity and long-term stability remain operator obligations. The final task receipt records checks on the frozen candidate; required CI remains the acceptance gate for blocked platform checks.

## Merge-queue investigation

Merge-group source `9e811d9c8e5196491352d14da3c64615861504b4` failed in [Windows job 117898971996](https://github.com/taskqorg/taskq/actions/runs/39202720692/job/117898971996). All 384 unit tests passed; `project_run_waits_for_the_host_owner` failed after 10.010 seconds. The child was still live, and its last committed phase was an open `taskq.operation.legacy.owner.read`. Store initialization reported about 3.7 seconds. This evidence does not distinguish CIM latency, scheduler connection, task enumeration or instance enumeration.

The initial owner decision retained the total 10-second deadline. Temporary native-read diagnostics now record those subphases and elapsed milliseconds in the owned fixture directory. A separate file preserves observations when the fixture kills the caller before its captured subprocess output returns. Reporting failure is ignored; all native identity checks, assertions and deadlines remain unchanged. No Windows service or host policy is changed. Remove this investigation instrument once native evidence identifies the responsible repair; a passing rerun alone does not prove the historical cause (R12).

The subsequent owner decision permits a bounded test-wait increase and moves the stalled native-read cause to [#1280](https://github.com/taskqorg/taskq/issues/1280). The fixture now allows 60 seconds per Windows invocation (10 seconds elsewhere), including startup, and logs elapsed time for both outcomes. All assertions, receipt deadlines and native-exit checks remain. The earlier 10.010-second failure is the before evidence; a passing widened-budget test does not prove the native delay repaired (R12, §7.4).
