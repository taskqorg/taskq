# Selective Mac recovery · #1285

Owner answer [6262705708](https://github.com/taskqorg/taskq/issues/1285#issuecomment-6262705708) accepts reconciliation evidence bound to execution and board revision; unknown effects remain blocked and native release remains mandatory. This implements E3 under R1, R11, R12 and R13; amended contract sections are §2, §3 and §5. R1/R13 wording is shortened without changing their authority.

## Behavior and retained protections

- Fresh local candidates show execution, board revision, native release, worktree status, current limits/access/focus and blockers. Nothing is preselected. The tray delegates to the owner CLI; workers cannot submit recovery.
- Selection carries owner-reviewed effect evidence. Exact fenced `requeue` receipts authorize only those tasks through the retained Stop hold. Ready tasks first park through a separately fenced operation. Unknown write outcomes reconcile the same immutable operation; newer owner intent stays held. Reopening exposes pending selections.
- Dirty and untracked files remain byte-for-byte intact in the adapter fixture. Paused projects, unselected tasks, operators, unresolved execution/input and publication remain blocked. Ordinary capacity, permission and runtime fences remain active. A new Stop revokes selected scope.
- Host supervision is submitted at most once through the existing installer, only after exact original Stop/host-exit evidence and an accepted selection. Submission is not reported as running. An uncertain installation is never replayed.

## Development failures and repairs

- `cargo +stable test --locked --bin taskq recovery::tests`: the first adapter run failed `paused projects must remain blocked after recovery` (3 passed, 1 failed). Separating retained selection from current admission fixed it; all six then-existing recovery checks passed. Selection protects existing work from the old Full stop while Pause continues to block new launches.
- The first full gate found `needs_gui_held_launch_rechecks_evidence_and_grant` (held count 0 instead of 1) and `resume_handoff_links_the_service_decision` (missing hold-refusal observation). Restoring the pass's hold snapshot and routing queue eligibility through the instrumented launch guard repairs both without changing their assertions.
- That gate's two incident-test failures were test-environment contamination: this runner set `TASKQ_INCIDENT_REPORTS=off`, making fixtures return disabled. The corrected isolated runner leaves that setting unset; production incident policy was not changed.
- Browser inspection found the evidence field too narrow. A block label and full-width input now retain the entered evidence visibly at 16/20px. The same browser instrument passes selection, required evidence, disabled unknown rows, identical retry payload, Mixed receipt and horizontal-overflow checks.
- First WKWebView snapshots showed an unconfirmed read: replacing the native IPC method did not intercept it. The snapshot-only native handler now supplies the invented review directly. The production bridge remains unchanged; fixture reads never contact the board.

## Candidate checks

Source commit: `54516f4018f22dd7e5bb4ce2b3ac51f3388de360` (rebased on `1676ec00614ba788b3f143263c79da91c853f271`). The later evidence commit changes reports/snapshot artifacts and extends the browser instrument to enter Recovery through the Stop page.

- `python3 scripts/check.py --base 1676ec00614ba788b3f143263c79da91c853f271`: **sandbox-blocked**, 159.626s / 900s. The full Rust run passed 400 main tests (31 ignored helpers), plus 16 integration tests. The sole failure was `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. This is not a passing canonical gate; merge-result CI remains required. The seven recovery tests and the unchanged GUI-hold/telemetry assertions passed.
- Remaining canonical stages were then run once individually. `cargo +stable run --locked -- selftest` passed 85 golden traces, text formats and local capture in an isolated offline installation; board read was explicitly unknown/offline. All Node component checks and the screenshot/release-script checks passed after the text repair below.
- `node tests/tray-action.test.cjs` initially failed its existing `ordinary Play cannot clear` assertion because the new sentence used `Ordinary`. Restoring the existing phrase with a semicolon passed all six groups without changing the assertion. This text-only repair followed the full Rust run; the Rust logic was unchanged.
- `node tests/tray-recovery.browser.cjs`: passed both themes, Stop-to-Recovery navigation, selection/evidence/unknown/duplicate-reconciliation and overflow checks. `git diff --check` passed. The final release build completed in 5m24s; its binary/source/image hashes are in [manifest.json](manifest.json). README images are refreshed with the mandatory `docs/shots/update.sh` source/blob manifest.

The isolated test subprocesses removed inherited worker/tracing variables and used temporary install stores. Production worker identity and permissions were retained for task operations. The unavailable sandbox test was not bypassed or reported as passed.

## UI evidence and limits

`native/` contains WKWebView snapshots from `taskq tray --snapshot DIR --route recovery,stop`, using isolated `docs/shots/fixture.sh` state plus the explicitly invented recovery fixture. Both themes at 16/20px were inspected. The recovery rows, disabled unknown checkbox, execution/revision, work status and evidence inputs render clearly. At 20px the bounded native viewport requires scrolling; full-page browser captures show the remaining confirmation controls. Native physical scrolling/keyboard acceptance is not claimed.

`dark-review.png`, `light-review.png`, `dark-mixed.png` and `light-mixed.png` come from the actual bundled UI instrument `node tests/tray-recovery.browser.cjs`. They show an invented selected row, an unknown disabled row and the partial-success receipt. They do not prove installed worker execution or external-effect correctness.

The three existing physical UI exceptions remain unchanged, as in [#1214 evidence](../stop-1214/report.md). No new exception is introduced. Owner visual acceptance, installed-client recovery and real native host reinstallation remain unverified. No production recovery, grant, model, profile, capacity or other-host change was performed. Rollback cannot undo board events or external writes; retain the Stop interlock and reconcile receipts rather than deleting it or downgrading through its floor.

Environment: macOS 27.0.1 arm64, rustc 1.98.1, Node v22.18.0; warm local build cache. CI timing and other platforms are not inferred from this run.

Final rebase adds only upstream `docs/evidence/1244` at `302d6c09`. Compiled inputs (`src`, Cargo/build/package files, scripts and fixtures) are identical to captured source `54516f4018f22dd7e5bb4ce2b3ac51f3388de360`; the published equivalent source commit is `bc221f93d2ec9be655063d2e3db9968ed15056f4`. Both manifests retain the actual build source and name this equivalence. No executable evidence was invalidated by that documentation-only rebase.

## Merge-group teardown repair

Owner answer 6263700734 authorizes shared test teardown repair within #1285. No production behavior or acceptance protection changes (R12, §7.4).

Before: [merge-group Windows run](https://github.com/taskqorg/taskq/actions/runs/39226989063/job/117975614332), candidate `59f161ce34a65e2d726f85abc7115fa2b921c962`, failed `historical_no_host_terminal_repair_requires_local_ownership` at its final `remove_dir_all` with OS error 32 (file used by another process). The suite reported 394 passed, 1 failed and 33 ignored. Mac and Linux passed. The failing file and holder were not captured; a specific handle owner is not established.

The fixture previously deleted each store inside the shared test process after dropping its local collector references. The repair runs the existing 22 ownership/archive scenarios in one bounded child and deletes its unique fixture root only after successful native process exit. This makes release of child-owned handles explicit and isolates the fixture from parallel test subscribers and subprocess creation. All ownership, unknown, idempotence, original-log and archive assertions remain; cleanup still must succeed. No retry, sleep, assertion deletion or ignored cleanup error is added. A failed child retains its directory for diagnosis.

After: `cargo +stable test --locked --bin taskq service::tests::historical_no_host_terminal_repair_requires_local_ownership -- --exact --nocapture` passed on macOS (1 parent, all 22 child scenarios, 0.42s execution; 14.58s warm build). Windows reproduction and after-proof remain pending CI; the Mac pass does not prove the Windows failure is eliminated. The process deadline is 60s inside the existing 900s suite budget.

Rebased repair checks: `python3 scripts/check.py --base origin/main` on base `beca44ed2b6971a782caaec5f7bd6423a32660c0` ran once, 52.427s / 900s. It passed 400 main tests (32 ignored helpers/qualifications) and 16 integration tests, including the repaired parent/child case. It then stopped on the unchanged `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. The canonical gate is **sandbox-blocked**, not PASS. Isolated `cargo +stable run --locked -- selftest` passed 85 traces, formats and local capture; board remained offline/unknown. `sh docs/shots/check.sh HEAD` passed. Unchanged Node/UI instruments retain the preceding evidence; they were not rerun for this test-only repair. Merge-result Windows CI and installed/owner UI acceptance remain open. No deployment or live recovery was performed.
