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

## UI evidence and limits

`native/` contains WKWebView snapshots from `taskq tray --snapshot DIR --route recovery,stop`, using isolated `docs/shots/fixture.sh` state plus the explicitly invented recovery fixture. Both themes at 16/20px were inspected. The recovery rows, disabled unknown checkbox, execution/revision, work status and evidence inputs render clearly. At 20px the bounded native viewport requires scrolling; full-page browser captures show the remaining confirmation controls. Native physical scrolling/keyboard acceptance is not claimed.

`dark-review.png`, `light-review.png`, `dark-mixed.png` and `light-mixed.png` come from the actual bundled UI instrument `node tests/tray-recovery.browser.cjs`. They show an invented selected row, an unknown disabled row and the partial-success receipt. They do not prove installed worker execution or external-effect correctness.

The three existing physical UI exceptions remain unchanged, as in [#1214 evidence](../stop-1214/report.md). No new exception is introduced. Owner visual acceptance, installed-client recovery and real native host reinstallation remain unverified. No production recovery, grant, model, profile, capacity or other-host change was performed. Rollback cannot undo board events or external writes; retain the Stop interlock and reconcile receipts rather than deleting it or downgrading through its floor.

Environment: macOS 27.0.1 arm64, rustc 1.98.1, Node v22.18.0; warm local build cache. CI timing and other platforms are not inferred from this run.
