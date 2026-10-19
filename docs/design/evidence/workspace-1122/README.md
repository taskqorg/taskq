# Native full-exchange workspace · #1122

The Mac workspace uses retained request/answer records and one editable native window. Home opens the selected exchange; opening and closing do not submit work. Follow-ups carry the selected project and leaf request. Confirmation carries the shown request/revision; partial and unknown outcomes remain explicit. R1, R9, R12, R13 and § 4 apply. No runtime, model, grants, working profile or other host was changed.

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
- `cargo test --locked`: 362 passed, 16 failed, 30 ignored in this worker. Fifteen failures involve worker-forbidden host-policy/install/lifecycle operations (sandbox-blocked). The remaining unchanged `service::tests::resume_handoff_links_the_service_decision` test passes with only `COREQ_TRACE_CONTEXT` removed from its test invocation. Worker identity and access restrictions were not removed. The full suite is not PASS; integration binaries after the failed unit binary were not reached.
- `cargo run --locked -- selftest`: 82 golden traces and formats passed; live completion failed twice with `legacy snapshot lock: timed out`. No profile/lock mutation or service restart was attempted. Full selftest remains unverified.

The GitHub publication gate remains green merge-result (`merge_group`) CI under the current task brief. This handoff is not a deployment or a claim that the whole C1–C8 design is implemented. Detach, archive, attachments and cancellation remain outside this slice.
