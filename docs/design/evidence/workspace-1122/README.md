# Native full-exchange workspace · #1122

The Mac workspace uses retained request/answer records and one editable native window. Home opens the selected exchange; opening and closing do not submit work. Follow-ups carry the selected project and leaf request. Confirmation carries the shown request/revision; partial and unknown outcomes remain explicit. R1, R9, R12, R13 and § 4 apply. No runtime, model, grants, working profile or other host was changed.

## Current acceptance

The [native check](https://github.com/taskqorg/taskq/pull/1275#issuecomment-6265950037) found that the workspace navigation handler blocked its own inline page. Reusing Home's navigation filter restored the transcript and revision controls. The same native window reopened with its position retained; request and answer hashes remained unchanged. A second tray process was refused. Snapshot rendering alone did not exercise the faulty builder.

The existing `own_page_loads_inline_html_only` regression failed with the old deny-all handler and passed with the shared filter (§ 7.4). It protects inline-page loading while retaining HTTP/HTTPS navigation refusal; no assertion was weakened. Native ownership and no-resend evidence above remains separate from this component check (R12).

The owner [accepted the appearance](https://github.com/taskqorg/taskq/issues/1122#issuecomment-6263678256) and later [waived the separate raise measurement](https://github.com/taskqorg/taskq/issues/1122#issuecomment-6266905779), reporting that the tray window opens and clicks work. Raise is owner-accepted, not measured. These decisions supersede the earlier pending acceptance statements below; green `merge_group` CI remains required before publication. No deployment is claimed.

Rebased onto `72fae8e1` with unchanged UI source blobs (the screenshot manifest still matches). Fresh candidate checks:

- `python3 scripts/check.py --base origin/main` (worker identity variables removed from test subprocesses): 405 unit tests passed; integration checks passed except the unchanged host-hold test, **sandbox-blocked** by `sandbox_apply: Operation not permitted`. Gate stopped after 68.535 seconds; it is not a full PASS.
- Remaining gate steps run separately: all `tests/*.test.cjs`, `node tests/workspace.browser.cjs`, screenshot-manifest and release-script checks passed. Offline selftest passed its local checks; board remained intentionally unknown.
- `cargo +stable run --locked -- selftest` passed against the live board, including no overdue nonlive terminal-record defects in the retained window. This supersedes the earlier live-selftest failure below; no host-profile repair was performed.

## Reply dispatch repair

Candidate source `aca86abae328f87d54e193f3b1a8b9f9b90e364e` fixes the review finding: Home Reply/Next previously saved a draft and acknowledged success without queuing the follow-up. The native Home handler now persists the draft, submits to the explicit project/current leaf, and only acknowledges accepted submission. Open/Review still selects without submitting or confirming. No contract decision changed (M7, A1, R12, § 7.4).

`cargo test --locked --bin taskq home_workspace_followup_dispatch -- --nocapture` failed before the fix with `Home acknowledged a reply without queuing it` (1 request instead of 2), then passed after. The regression preserves target isolation, stale-click rejection, refused drafts and reopen-without-resend protection; no existing assertions were weakened.

Fresh checks:

- `env -u TASKQ_ATTEMPT -u TASKQ_TASK -u TASKQ_ROLE python3 scripts/check.py --base origin/main`: 395 unit tests passed, 31 ignored; host policy/restart, resource contract and service-log integrations passed. The telemetry process test remains **sandbox-blocked**: `sandbox-exec: sandbox_apply: Operation not permitted`. The first run inherited worker variables and had 16 host/lifecycle failures; only the test subprocess environment was isolated, with no sandbox or permission change.
- All 18 `tests/*.test.cjs` scripts, `node tests/workspace.browser.cjs`, screenshot manifest tests and release script checks passed.
- Isolated offline selftest: 85 golden traces, formats and local telemetry passed; board intentionally unknown. Live `cargo run --locked -- selftest`: failed with one terminal record older than TTL 300s; the host profile was not changed.
- `docs/shots/update.sh` passed and refreshed the source manifest. [Fresh native workspace capture](review-fix-workspace-dark-20.png) was inspected: full reply, target/effect and revision control are visible. Release artifact SHA-256: `9685efa7ce1e8b297088f913489a3abe873d361f83eb500af4001c0438013f28`; same platform/rendering settings as below, 860×680 logical pixels, dark 20 px. The first temporary fixture mixed `/var` and `/private/var` root aliases and showed no selection; recreating the fixture with one canonical root restored the intended exchange.
- Interactive native ownership remains **unverified**. The isolated bundle reported WebKit sandbox extension `Operation not permitted`; Computer Use timed out and could not resolve the app. No reopen/raise/close sequence was observed. The isolated fixture tray was stopped; no working service was changed.

Owner visual acceptance of the linked before/after evidence, native ownership qualification and green `merge_group` CI remain required before publication. No CI wait, merge or deployment was performed.

## Before and after

Baseline `bf41d2f6`: `node tests/tray-workspace.test.cjs` failed because Home had no `ai-open` route. The baseline Mac capture shows the inline exchange and confirmation. The same regression now passes. Native after captures show the full exchange, target/effect review, revision-bound Confirm and narrow conversation navigation.

- [Before: Home, dark, 16 px](before-home-dark-16.png)
- [After: Home, dark, 16 px](after-home-dark-16.png)
- [After: workspace, dark, 20 px](after-workspace-dark-20.png)
- [After: workspace, light, 16 px](after-workspace-light-16.png)
- [After: narrow workspace, dark, 20 px](after-workspace-narrow-dark-20.png)

Images use the same invented request and proposal. These are native WKWebView captures with an isolated installation and no running service, not a live agent or board mutation. Artifact identity and environment are in [capture.json](capture.json). Capture source is `bee2b29b`; the later corrupt-preference/parent validation change does not affect this valid fixture or renderer. Its Rust regression was rerun separately. Owner visual acceptance remains required; installed-service window reopening and live proposal application were not exercised.

Reproduce in an OS temporary directory: run `docs/shots/fixture.sh DIR`, then `python3 docs/design/evidence/workspace-1122/fixture.py DIR`. Use `TASKQ_INSTALL=DIR/install target/release/taskq tray --snapshot OUT --route home`, then a separate invocation with `--route workspace`. For the narrow capture set the fixture's `install/tray.json` to `{"size":[360,580]}`; the normal workspace capture is 860×680 logical pixels. Both themes and 16/20 px are captured. No working installation is used.

## Checks and limitations

- `node tests/workspace.browser.cjs`: passed full replies, two independent drafts, selection/history without resending, exact revision, double-click suppression, partial readbacks, IME, 360/860 px and 16/20 px. This is a browser/IPC instrument, not native window-ownership proof.
- `node tests/tray-workspace.test.cjs`, `node tests/tray-reply.test.cjs`, `node tests/tray-next.test.cjs`, `node scripts/check-web.cjs`: passed. The Next test now requires the explicit project as well as the original reply token and parent request; no assertions were removed.
- `cargo test --locked --bin taskq conversations::tests::exchange_identity_drafts_and_confirmation -- --exact`: passed request-backed isolation, reopening without new requests, retention past 20 command-history entries, current-leaf checks, stale/duplicate confirmations, partial-state refusal and corrupt-preference preservation.
- `cargo test --locked --bin taskq tray::tests::snapshot_routes_capture_only_named_pages -- --exact`: passed. The existing matrix assertion now excludes the separate workspace renderer; explicit workspace coverage was added, and all prior route assertions remain.
- Rebased on current `main`: `python3 scripts/check.py --base origin/main` ran every full-scope step. `cargo test --locked --no-fail-fast` passed all binaries except `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, which fails with `sandbox-exec: sandbox_apply: Operation not permitted` inside the worker sandbox (sandbox-blocked, unchanged by this branch). Offline selftest in a temporary installation passed; all `tests/*.test.cjs`, `docs/shots/check.test.sh` and the release scripts passed.
- The rebase found two stale Home label assertions (`Confirm: apply N changes`); on Mac, Home now routes proposals to the workspace with `Review changes: N changes`. Both tests assert the new label with the same placement check; the Windows label stays in `src/tray-app.js`.
- Live `cargo run --locked -- selftest` failed on host state: `attempt terminal records: FAIL (1 older than ttl 300s)`. That is the host profile store, not this diff; it remains unverified.

The GitHub publication gate remains green merge-result (`merge_group`) CI under the current task brief. This handoff is not a deployment or a claim that the whole C1–C8 design is implemented. Detach, archive, attachments and cancellation remain outside this slice.
