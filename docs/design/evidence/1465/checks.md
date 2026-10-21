# Workspace operation reviews — #1465

Implements the approved O placement from the #1443 owner plan. R9, R11, R12,
§ 6 and § 7 remain unchanged. No rule is amended.

Menu launchers open the W workspace with an operation and optional exact root.
The existing tray controller renders the reviews there; its resource editor,
immutable capacity request, recovery selection and lifecycle receipt validators
remain the executors. The workspace native bridge forwards only the named
operation actions to the existing dispatcher and ActionQueues. Replies and picker
results return to the workspace. Native folder selection, grants, registration
and setup exact-effects approval remain native-owned.

## Failure and retained protection

Base: `2f6a2d2c9ac427f9687b64714bd2cfd21805773c`.
`node tests/workspace-operations.browser.cjs` failed before the change:
`A1: no emergency confirmation remains in menu`, actual 1, expected 0.
The same check passes after the change. A lifecycle unknown previously permitted
a new operation ID; it now remains fenced until the exact native readback.
Setup application also names the native picker's generation and is single-use.
Update success requires fresh readback from the original project, not an equal
version from another project. No timeout retries an effect.

The existing Stop, recovery and resource browser instruments now run the same
assertions in the workspace. They retain exact scope, original instance/expiry,
Drain versus Full stop, unknown release, mixed receipts, unchanged selection on
reconciliation, revision conflicts, input focus, exact saved/effective revision,
foreign receipts and legacy refusal. Component tests use the same controller in
workspace mode; the menu navigation test instead verifies a launcher with no
local Work mode or effect. Notification, scroll/filter/Back and simple setting
checks remain.

## Checks

- `npm run build`: passed, including the module/effect inventory.
- `node tests/workspace-operations.browser.cjs`: passed. Opening/Back/cancel,
  unavailable target, exact Full stop, single-use/unknown fencing, update target
  and freshness, native-picker generation, admission and keyboard exits.
- `node tests/tray-stop.browser.cjs`: passed in dark 16 px and light 20 px.
- `node tests/tray-recovery.browser.cjs`: passed in dark 16 px and light 20 px.
- `node tests/tray-resources.browser.cjs`: passed in both themes, 16/20 px and
  280/320/360 px widths; native effects are substituted.
- `node tests/workspace-communication.browser.cjs`: passed; W drafts, caret,
  IME, Send/Enter, unknown same-ID reconciliation and answer identity retained.
- `node tests/tray-view-state.browser.cjs`: passed; menu restart, Back,
  filters, scroll and unavailable destinations retained.
- Initial full `python3 scripts/check.py --base 2f6a2d2c9ac427f9687b64714bd2cfd21805773c`:
  failed after 234.881 s; Rust 444 passed, 18 failed, 34 ignored.
  Explicit worker refusals (`a worker cannot change host policy`, install,
  restart/Stop) are sandbox-blocked. Other host/recovery assertion failures
  remain unqualified. No authority environment or protection was relaxed.
  This is not a green gate. Final-candidate checks are recorded below.

## Visual evidence and limits

- [Before: Stop review in menu](stop-menu-before.png): Chromium baseline bundle,
  320 × 900, dark 16 px, invented fixture.
- [After: Stop review in workspace](stop-workspace.png): product workspace bundle,
  800 × 600, dark 16 px, the same invented operation and project.

Both images were opened and inspected individually at their actual pixel size:
review text and essential buttons are visible without horizontal clipping.
The browser capture does not establish native effect qualification. Owner layout
acceptance is recorded below.
No production Stop, recovery, policy change, update or project registration was
performed. WKWebView/WebView2 operation dispatch, native picker window ownership
remain unverified. Owner actual-size layout review is accepted below. GitHub publication still needs
green merge-result `merge_group` CI (§ 7); this worker does not wait for CI.

## Final candidate

Frozen implementation: `a6a69fa9a8603da70533f4aa747fc40a1eb6194f`; rebased on
`origin/main` at `2f6a2d2c9ac427f9687b64714bd2cfd21805773c`.
The final canonical gate again failed: Rust 444 passed, 18 failed, 34 ignored;
163.168 s including setup/build. The 18 failing names match the initial run.
Worker host-authority refusals remain sandbox-blocked; other assertions are
unqualified. The gate stopped at Rust, so downstream checks are separate evidence.

All 17 `tests/*.test.cjs` component checks passed separately. The isolated
`TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest` passed
97 fold traces, formats and staged private telemetry capture. Board state was
intentionally not read; neither the offline selftest nor these fixtures perform
a production operation.

`docs/shots/update.sh` completed from the frozen implementation in an isolated
installation. Its WKWebView project-menu and summary-icon captures were opened
and inspected; the menu has launchers rather than confirmation controls.
The committed native manifest records the exact source, binary, UI blobs and
image hashes. These two README images plus the two placement images above make
four images in this review set (V38). They qualify native menu rendering only;
workspace dispatch, native picker ownership and actual effects remain unverified.
`sh docs/shots/check.test.sh` passed the fresh/stale/malformed/missing provenance
cases. The owner accepted this four-image layout in [answer 6320399983](https://github.com/taskqorg/taskq/issues/1465#issuecomment-6320399983):
“Accept this layout; retain native operation and picker qualification as release work.”
This closes the A8/V38 visual decision; it does not qualify native effects or
replace green merge-result CI (R12/§ 7).

## Windows publication repair

The [merge-group run](https://github.com/taskqorg/taskq/actions/runs/39631644429)
at `ed12df70eb2cd4e5e46deb9291edfa0539c7e8d2` failed on Windows:
`detached_install_observers_share_one_acknowledgement_and_cannot_create_a_command`
completed its installation assertions, then fixture directory removal returned
Windows error 32 (file in use). Linux, macOS and Windows GNU jobs passed.

The receipt worker notified settlement before dropping its tracing dispatcher.
That dispatcher can retain the collector and its SQLite writer after the fixture
has settled and dropped its own capture. Release it before notifying settlement;
the FIFO, immutable command identity, required checkpoint and error handling stay
unchanged. The installer test keeps every scope, generation, replay, ownership
and unchanged-file assertion. R9, R11, R12 and § 6 are unchanged; no rule is amended.

`telemetry::tests::receipt_settlement_waits_for_dispatch_release` holds dispatcher
destruction open and checks that settlement cannot finish until it is released.
This is a worker/collector teardown check, not native installation qualification.
The accepted four-image layout and owner answer remain unchanged.

Repair source: `a3ea38893f6d14086ba7359c6f8596267a2cfc4c`, rebased on
`ab52a687db2a69d4c4c7f20accd67be5a4430298` (the CoreQ writer-promotion pin).
Local tools: Rust 1.99.0 (Windows GNU), Node 24.5.0 and Python 3.12.10.

- Before the fix, `cargo +stable test --locked --bin taskq telemetry::tests::receipt_settlement_waits_for_dispatch_release -- --exact --nocapture`
  failed: `receipt settled while its dispatcher still owned the collector`.
  The check runs in its own process so unrelated queued receipts cannot hide it.
- After the fix, the same regression passed inside the selected full gate:
  `python3 scripts/check.py --base ab52a687db2a69d4c4c7f20accd67be5a4430298`.
  The gate itself failed: 437 passed, 20 failed, 35 ignored; 271.687 s / 900 s,
  no retries, Windows GNU with warm dependencies. Explicit worker refusals for
  host installation/policy, lifecycle and Windows security restoration are
  sandbox-blocked. Other assertion failures, including publication call counts,
  remain unqualified. Rust integration checks were not reached; this is not a
  green gate. The original installer check is sandbox-blocked here, so its
  post-repair Windows MSVC cleanup outcome still needs merge-group CI.
- `npm run build` and all 17 `tests/*.test.cjs` component checks passed.
- All six browser instruments passed on Windows Chromium headless:
  `workspace-operations`, `tray-stop`, `tray-recovery`, `tray-resources`,
  `workspace-communication` and `tray-view-state` (`tests/*.browser.cjs`).
  V15/V28 protections remain; effects are substituted, not native-qualified.
- `cargo +stable run --locked -- selftest` passed in an isolated temporary
  installation with `TASKQ_SELFTEST_OFFLINE=1`: 97 traces, formats and staged
  capture. Board status stayed explicitly unknown; no running writer was used.
- `bash docs/shots/check.sh` (Git Bash) and `git diff --check` passed.

No authority environment was relaxed, no production operation was applied and
no new review images were added. Publication still requires green `merge_group`
CI on the merge result (R12/§ 7).

## Publication conflict repair

R12/§ 7: PR #1508 at `d38cae5179a63799c28311bab763abff3c4750dc`
reported `CONFLICTING`. Rebase onto `7cc123790fd1fd5612a112fb05abe3c777840f09`
reproduced conflicts in the screenshot manifest and summary icon. The workspace
implementation, owner acceptance and Windows receipt regression were retained.
No rule or test assertion changed.

Rebased implementation: `5ce2f440448a23f21d339d49212dfd877d758208`.
Before regeneration, `sh docs/shots/check.sh` exited 2 for four stale UI blobs.
After `sh docs/shots/update.sh`, that same check passed, as did
`sh docs/shots/check.test.sh` and `git diff --check`. The regenerated WKWebView
project menu and summary icon were opened individually and inspected: labels,
launchers and icon counts remain visible without clipping. These replace the
two native images in the existing four-image review set; no distinct-gap image
was added. Workspace layout and its recorded owner acceptance are unchanged.

- `python3 scripts/check.py --base 7cc123790fd1fd5612a112fb05abe3c777840f09`:
  **not green**, macOS arm64, warm dependencies; 446 passed, 18 failed,
  34 ignored; 155.017 s / 900 s, no retries. Explicit worker host installation,
  policy and lifecycle refusals are sandbox-blocked. Other assertion failures
  remain unqualified. Rust integration checks were not reached.
- `telemetry::tests::receipt_settlement_waits_for_dispatch_release` passed in
  that gate; the Windows teardown fix and all its assertions remain unchanged.
- `npm run build`, all 17 Node component scripts and all six affected browser
  instruments passed. V15/V28 scope, identity, unknown receipt and live-work
  protection remain; browser effects are substituted.
- `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest` passed in an
  isolated temporary installation: 97 traces, formats and staged capture.
  Board observation remained unknown by design.

Native workspace dispatch, picker ownership and actual effects remain release
work under owner answer 6320399983. Publication still requires green merge-result
`merge_group` CI; this repair does not claim a green local gate or deployment.
