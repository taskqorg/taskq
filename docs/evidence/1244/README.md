# Six-board merge queue activation

Task #1244 applies R12, R13 and sections 7/9. Queue activation is verified; two-task live qualification remains incomplete.

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

Task #1215 / PR #1274 retains reviewed head `ba2bf49939e914f6532f0f40e1c04b0c02fae337`. After activation the service recorded [enqueue receipt](https://github.com/taskqorg/taskq/issues/1215#issuecomment-6259280762). Its first observed merge group is `f3be1b899a1ce4976bd404e5482f5d847d65d1f1`, [CI run](https://github.com/taskqorg/taskq/actions/runs/39196854408). Enqueue is not a merge or close receipt (R12).

The other prepared task #1149 / PR #1272 is parked in `later` after the old context mismatch. Its unblock condition now has verified settings readback. The owning operator must resume it through TaskQ; this worker must not bypass attempt fencing, hand-edit task logs, or publish another task outside its lifecycle. Two successive merges and correlated close receipts remain unproved. No CI wait or synthetic qualification task is introduced.

The release workflow runs on main pushes and version tags, not merge_group. A main push with an unchanged Cargo version should skip artifact publication; a version change must pass exact-source checks and staged selftests before publication. This is workflow inspection, not live post-queue release proof. No new release or deployment is claimed.

## Rollback

`*.rollback.json` retain each original writable ruleset payload. Before rollback, GET and reconcile current state; PUT the matching rollback payload and independently GET it back. Keep merge_group workflow triggers while queued PRs remain. Restoring the original TaskQ/CoreQ contexts would restore the known docs-only mismatch. Rollback cannot undo an already merged PR or published release.

## Verification

Existing #1243 regression checks cover queue admission/readback, observed merge-result CI, unknown outcomes, publication restart and main-move retry accounting. No assertions were weakened. A passing model/adapter check does not qualify native queue delivery.

- `npm ci --ignore-scripts --no-audit --no-fund`: passed; repaired missing local esbuild prerequisite, without dependency changes.
- `cargo test --locked`: failed, 361 passed, 16 failed, 30 ignored. Host mutation tests encounter worker guards (sandbox-blocked, not PASS); `service::tests::resume_handoff_links_the_service_decision` also has a trace-link assertion mismatch. These unchanged-code failures were not repaired or hidden.
- `cargo test --locked --bin taskq queue`: 7 passed, including expected-head enqueue/readback, merge-result CI and unknown-result protection.
- `cargo test --locked --bin taskq golden`: 5 passed, including main-move retry traces and section 2 coverage.
- `cargo test --locked --bin taskq pending_publication_survives_restart_without_a_reviewer`: 1 passed.
- `cargo run -- selftest`: 82 golden traces and formats passed; live check failed with `legacy snapshot lock: timed out`. Live selftest is not PASS.
- `python3 scripts/check.test.py` and `git diff --check`: passed.

The tested implementation, dependencies and assertions are unchanged from main `bf41d2f673951a6d19e81c39a8d72ad9cf9daf77`; this branch changes rollout evidence only. Merge-result CI remains the publication gate. No CI wait was performed.
