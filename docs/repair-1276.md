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

## Follow-up diagnostic (#1280)

The historical failure records store-open phases of 221 µs initialization, 738543 µs write, 156028 µs connect, 833 µs check, 1159691 µs configure, 62 µs checkpoint and 1674316 µs schema work: 3.730 seconds total. The last committed method start identifies the native owner read, not the subphase responsible for its latency. No artifacts were retained by that run.

[Merge-result Windows job 117906961401](https://github.com/taskqorg/taskq/actions/runs/39205368622/job/117906961401) passed with the 60-second fixture budget. Its log contains no successful-test phase output; the fixture deletes its local trace after success. Neither this pass nor the earlier open span establishes the cause or a sufficient minimal bound (R12).

A temporary diagnostic step on PR [#1283](https://github.com/taskqorg/taskq/pull/1283) reran the fixture with `--nocapture` in [Windows job 117916974527](https://github.com/taskqorg/taskq/actions/runs/39208656944/job/117916974527). Both invocations observed the waiting owner, in 0.56 s and 1.52 s. Inside the PowerShell owner read, `Get-CimInstance` for the own process took 130 ms, the Task Scheduler connect 13 ms, the task listing 12 ms and the binding scan 10 ms: about 205 ms in total. The canonical run of the same job passed the fixture in about 2.6 s for both directories.

Conclusion: the native read has no deterministic stall. The failed run spent 3.73 s opening the telemetry store, where a healthy open adds well under a second, and its PowerShell owner read was still running more than 6.2 s later. Both phases were more than ten times slower than healthy runs on the same job. In the same failed job, the two other `host_policy` tests also took 20 s instead of 12 s. The cause is intermittent latency of the Windows runner during cold process, SQLite and CIM work, outside taskq code. The failed run kept no native subphase trace, so the slow subphase inside that read stays unknown (R12). One of the nine merge-group runs on that day failed this way.

Decision: no product deadline changes, and the fixture keeps the 60-second Windows bound from #1276. A 10-second bound already failed once on a slow runner at 10.010 s, and healthy runs need under 3 s. A 60-second bound gives a margin of more than twenty times over a healthy run, and a real hang still fails. The temporary diagnostic step and the success-path trace print are removed; the failure path still reports committed phases and the native trace.
