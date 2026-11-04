# Executor identity at slot release

R12; #2337 A1/A2. Retirement must retain the known executor PID after exit; absent identity stays unknown. This fixes the existing contract without changing a rule or host policy.

## Before and after

`cargo test --locked --bin taskq worker_lifecycle_telemetry_and_status_readback -- --nocapture`

Before the fix, the strengthened existing lifecycle check failed: `slot-release must retain executor identity after release`, actual `0`, expected `477` (the fixture process PID). After the fix, the same check passed. It also checks every release observation for that attempt, protecting against a duplicate identity-free reservation release.

`python3 scripts/telemetry.test.py`

Before the validator fix, a complete lifecycle with release PID `0` incorrectly printed `verdict: PASS (spawn to close traced for every attempt)`. After the fix, complete identity passes; zero, missing, sentinel, string, or unknown required identity fails. This check executes the actual Python validator embedded in `scripts/telemetry.sh`; it does not qualify the installed export command.

## Retained protection

- `cargo test --locked --bin taskq native_checkpoint_failure_retains_execution_and_retries_only_storage`: passed. Real owned-process teardown through reduced reap retains its PID even with corrupt recovery storage; existing checkpoint, ownership, no-replay and exit assertions remain.
- `cargo test --locked --bin taskq retirement_retains_recovery_pid`: passed. An exact persisted task/attempt/execution retains its historical PID without an OS liveness query; another execution or missing file records unknown.
- `cargo test --locked --bin taskq boot_after_intent_releases_only_this_hosts_older_executions`: passed. Boot release still requires later-boot proof for this host; foreign, earlier and failed boot observations remain fenced.

These are local Linux source/process checks. They do not qualify pds18 installed telemetry or historical exports. Live qualification remains with the pds18 proof task after release; merge-result CI remains the publication gate (§7, R12).

## Candidate gate fixture repair

The first `python3 scripts/check.py` run failed after 154.378 s: 905 unit checks passed, 37 ignored, one failed. `runtime::app_control::tests::cancellation_is_owner_bound_bounded_and_never_replays_work` rejected its supposedly trusted endpoint with `Codex App IPC ownership/protection invalid`. The fixture directory was mode `775` on this host. It now explicitly sets its owned directory to `700` before the exchange. Product ownership validation, all cancellation assertions and the deliberate directory/socket/symlink refusal cases remain unchanged.

`cargo test --locked --bin taskq cancellation_is_owner_bound_bounded_and_never_replays_work -- --nocapture` passed after this fixture repair.
