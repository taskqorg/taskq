# Six-board merge queue activation

Task #1244 applies R12, R13 and sections 7/9. A1–A4 are verified within the bounds below. Six queues are active; three real TaskQ deliveries have merge-result CI and close receipts.

## Authorization and cutover

The owner approved taskqorg/{taskq,agentq,coreq,dashq,serverq,specq} and reuse of the merged #1243 implementation. No implementation, test, runtime grant or model setting changed here.

- [Mac activation](https://github.com/taskqorg/taskq/issues/1233#issuecomment-6258723768): exact signed 4.6.19 artifact, one native service, six roots, health and live selftests.
- [Legacy retirement](https://github.com/taskqorg/taskq/issues/1244#issuecomment-6259007620): Windows retirement receipt retained; FRA native services/autostarts removed and zero TaskQ processes observed. Neither host may be restored by this task.
- [Credential capability restored](https://github.com/taskqorg/taskq/issues/1244#issuecomment-6259253924): owner updated the existing Mac PAT with Administration write for the same six repositories. No login or credential fallback was used.

## Before and after

`activation-blocked.json` preserves the prior agentq PUT failure (HTTP 403); all protections remained unchanged then. `activation-6259257305-before.json` contains fresh ruleset reconciliation against each saved rollback payload and main workflow content/blob identities. The five workflow PRs in `workflow-merges.json` were already merged; no workflow work was repeated.

`activation-6259257305-after.json` records successful PUT followed by independent ruleset GET and effective `/rules/branches/main` GET for all six repositories. Each ruleset is active with an empty bypass list, deletion and non-fast-forward protection, and an ALLGREEN squash queue (two builds, one PR per merge, 60-minute timeout). Strict head freshness is replaced by merge-result CI.

TaskQ and CoreQ already use an always-running aggregate `tests` job. It requires successful planning and all checks selected by the accepted scope policy; TaskQ also requires shots and, for full changes, Windows GNU. The previous rules demanded absent Rust matrix contexts on docs-only PRs. Their required context is now `tests` from GitHub Actions (integration 15368). No test job or assertion was removed; code changes still require the selected platform matrix. Other repositories retain every previous required context. Full workflow contents are preserved in the before evidence.

## Live qualification

`qualification-6259455299.json` records fresh GitHub GETs: all six active rulesets match the activation readback, effective main rules retain the protections, and all six main CI workflows include merge_group. No settings were written during this final verification.

| Task / PR | Merge time (UTC, 2026-10-19) | Merge SHA | Merge-group CI | Close receipt |
|---|---|---|---|---|
| #1215 / #1274 | 04:00:21 | `f3be1b899a1ce4976bd404e5482f5d847d65d1f1` | [39196854408](https://github.com/taskqorg/taskq/actions/runs/39196854408), success | [6259432053](https://github.com/taskqorg/taskq/issues/1215#issuecomment-6259432053) |
| #1149 / #1272 | 04:03:00 | `b2cd93869b3b56c3fc5447d5bd610b43ec090c9d` | [39197505639](https://github.com/taskqorg/taskq/actions/runs/39197505639), success | [6259389462](https://github.com/taskqorg/taskq/issues/1149#issuecomment-6259389462) |
| #1124 / #1129 | 04:40:41 | `d71c3659b91191b385b277a3d6f90ee4b9111a8e` | [39200115890](https://github.com/taskqorg/taskq/actions/runs/39200115890), success | [6259868984](https://github.com/taskqorg/taskq/issues/1124#issuecomment-6259868984) |

#1124 was enqueued at 04:33:48 ([6259726985](https://github.com/taskqorg/taskq/issues/1124#issuecomment-6259726985)); its log has no requeue between enqueue and close. Its main-push release run [39200670276](https://github.com/taskqorg/taskq/actions/runs/39200670276) also passed tag detection and skipped checks, build, attest and publish.

Both CI runs have event `merge_group` and head SHA equal to the observed merge SHA. The task logs contain no requeue between enqueue and close. The reviewed heads remain `ba2bf49939e914f6532f0f40e1c04b0c02fae337` and `6870d8040dfb78287ff36b54171b99f74b44c514`; the older #1149 context-mismatch requeue preceded activation and is preserved in `live-6259257305.json`. Close receipts arrived in reverse order; their delivery lines identify the correct merges. Their saved verdict bodies still say CI pending, but the final delivery receipts and fresh run GETs establish green merge-result CI.

Release runs [39197336843](https://github.com/taskqorg/taskq/actions/runs/39197336843) and [39197549786](https://github.com/taskqorg/taskq/actions/runs/39197549786) were triggered by main pushes on those exact merge SHAs. Both succeeded: tag detection passed; checks, build, attest and publish were skipped. This proves the unchanged-version queue path does not publish another artifact. A version-changing queue delivery, subsequent artifact attestation and deployment are not qualified by these two deliveries. No new release or deployment is claimed. Live successive delivery was qualified on TaskQ; other boards have configuration/workflow readback, not synthetic task deliveries.

## Rollback

`*.rollback.json` retain each original writable ruleset payload. Before rollback, GET and reconcile current state; PUT the matching rollback payload and independently GET it back. Keep merge_group workflow triggers while queued PRs remain. Restoring the original TaskQ/CoreQ contexts would restore the known docs-only mismatch. Rollback cannot undo an already merged PR or published release.

## Verification

### Earlier candidate (retained failure evidence)

Existing #1243 regression checks cover queue admission/readback, observed merge-result CI, unknown outcomes, publication restart and main-move retry accounting. No assertions were weakened. A passing model/adapter check does not qualify native queue delivery.

- `npm ci --ignore-scripts --no-audit --no-fund`: passed; repaired missing local esbuild prerequisite, without dependency changes.
- `cargo test --locked`: failed, 361 passed, 16 failed, 30 ignored. Host mutation tests encounter worker guards (sandbox-blocked, not PASS); `service::tests::resume_handoff_links_the_service_decision` also has a trace-link assertion mismatch. These unchanged-code failures were not repaired or hidden.
- `cargo test --locked --bin taskq queue`: 7 passed, including expected-head enqueue/readback, merge-result CI and unknown-result protection.
- `cargo test --locked --bin taskq golden`: 5 passed, including main-move retry traces and section 2 coverage.
- `cargo test --locked --bin taskq pending_publication_survives_restart_without_a_reviewer`: 1 passed.
- `cargo run -- selftest`: 82 golden traces and formats passed; live check failed with `legacy snapshot lock: timed out`. Live selftest is not PASS.
- `python3 scripts/check.test.py` and `git diff --check`: passed.

These earlier results apply to main `bf41d2f673951a6d19e81c39a8d72ad9cf9daf77`. Final verification rebases the evidence-only branch onto `b2cd93869b3b56c3fc5447d5bd610b43ec090c9d` and reruns affected checks. No assertions or rule decisions changed. Merge-result CI remains the publication gate; the worker does not wait for CI.

### Final candidate

The implementation base is `b2cd93869b3b56c3fc5447d5bd610b43ec090c9d`; this branch adds evidence only. Commands ran after rebase, without removing worker identity or changing grants:

- `cargo test --locked --bin taskq queue`: 7 passed.
- `cargo test --locked --bin taskq golden`: 5 passed, including main-move retry traces.
- `cargo test --locked --bin taskq pending_publication_survives_restart_without_a_reviewer`: 1 passed.
- `cargo test --locked`: 363 passed, 16 failed, 30 ignored. Host-mutation cases remain sandbox-blocked by worker guards; the existing resume trace-link assertion also fails. Full suite is not PASS.
- `cargo run -- selftest`: 82 golden traces and formats passed; live check failed with `legacy snapshot lock: timed out`. Board selftest is unknown, not PASS.
- `sh .github/release-tag.test.sh`: 10 cases passed, including unchanged versions and failed prior-version reads.
- `python3 .github/release-check.test.py`: passed exact-source, freshness and latest failure/pending isolation.
- `python3 scripts/check.test.py` and `git diff --check`: passed.

### Rebased candidate

The branch is rebased onto `1903ed61f69f93a36752a19c3cfcd11b512c4fa6` and keeps only `docs/evidence/1244/`; the earlier test-fixture commit `bd4c9f71` is dropped. Results for this candidate are in the PR's merge-result CI and the task result.

A1/A2 have fresh API proof; A3 has affected regression proof; A4 has three real merge/close receipts and observed no-release workflow behavior. These do not replace the merge-result CI gate for PR #1266. No runtime, workflow, test or contract changes are included; no weaker assertion or protection was introduced.
