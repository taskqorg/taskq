# README screenshots after the operator surfaces

Task #1015 refreshes the two essential README images: the billing task list and the full storefront exchange. Existing scenarios, recorded examples, setup and release-verification text remain unchanged. The examples are synthetic; no board mutation, completed agent run or native lifecycle qualification is claimed.

## Candidate and release

The published v4.6.23 tag is `c8197b1aa7c4cf914ef296ea8d2f20ef46c427c3`. Both dependency merges, `16096c82` (#1122) and `41406f9a` (#1148), are ancestors (`git merge-base --is-ancestor`, exit 0). The earlier v4.6.22 blocker is resolved. Capture provenance is in [the manifest](../../manifest.json).

The candidate changes only snapshot framing in the renderer: a project capture leaves Service collapsed. Ordinary navigation is unchanged. The workspace renderer is identical to the published release. The retained billing fixture is selected first so the task list fits; the retained #1122 workspace fixture supplies the invented proposal. No real service runs in the isolated installation.

## Before and after

- [Before page](before.html), [after page](after.html): GitHub Markdown API output with the retained local CSS, not a capture of GitHub's website.
- [Rejected task-list capture](rejected-project-clipped.png): the released snapshot path forces Service open and clips the lower controls. The candidate collapses only that snapshot disclosure.
- [Rejected workspace capture](rejected-workspace-path-alias.png): mixed temporary-root aliases leave the exchange unselected. The pipeline now canonicalizes the temporary root before both fixtures.
- The final PNGs remain unedited native WKWebView outputs. Display widths retain 16 px text on desktop; narrow README layouts scale the workspace image and link its full-size original for reading.

R12 keeps these image checks separate from live operation. No assertion, golden trace, permission, model, working profile or runtime policy changed. Rules R7, R8, R12 and § 7 apply; no rule amendment is required.

The owner accepted these visuals in [answer 1](https://github.com/taskqorg/taskq/issues/1015#issuecomment-6281097452), including the narrow-page link to the readable original. A4 visual acceptance is satisfied; reviewer acceptance and publication CI remain separate (§ 7).

## Visual inspection

The final native task list shows all three tasks and Home/Settings without the previous clipping. The workspace shows the complete request, reply, proposed target/effect and Confirm revision 2. Desktop (960 px) and narrow (390 px) page renderings loaded every image with no horizontal page overflow. Header and setup phrases remain intact; code blocks retain their own horizontal scrolling. At 390 px the workspace is an overview: open the linked original to read its text. This is a stated limitation, not mobile-native qualification.

- [Desktop queue](after-960-queue.png) and [exchange](after-960-exchange.png).
- [Narrow queue](after-390-queue.png) and [exchange](after-390-exchange.png).
- [Before desktop](before-960-queue.png) and [before narrow](before-390-queue.png).

`docs/shots/update.sh`, `docs/shots/check.sh`, manifest image/source hash verification, `sh -n docs/shots/update.sh` and all 18 `tests/*.test.cjs` scripts passed. No tests or assertions changed. The clipping failure and its replacement native capture above are the before/after proof for the snapshot-only repair (§ 7.4).

## Initial candidate checks

- `cargo test --locked` (worker identity variables removed only from test subprocesses): 409 unit tests passed, 34 ignored. Host policy, restart, resource and service-log integrations passed. Telemetry integration had 7 passed, 1 ignored and 1 **sandbox-blocked**: `resume_records_its_result_without_releasing_the_host_hold` reports `sandbox-exec: sandbox_apply: Operation not permitted`. The suite is not a full PASS; no permission change or retry bypass was attempted.
- `cargo run --locked -- selftest`: passed 85 golden traces, format checks, fresh board read, no overdue nonlive terminal defects and committed local telemetry.
- Green merge-result (`merge_group`) CI remains the publication gate under § 7. CI was not awaited. No merge or deployment occurred.

## Accepted handoff after rebase

The branch was rebased onto `73f3ac1f`; every accepted image hash and manifest UI source blob remains unchanged. No new visual acceptance is needed. The upstream README agent-routing sentence changed; the retained page renders document the accepted visual candidate before that unrelated text update.

- Fresh `cargo test --locked`: 408 passed, 1 failed, 34 ignored. The unchanged `profile::tests::concurrent_appenders_keep_records` failed its flush-error-prefix assertion. Its isolated exact rerun passed; this does not turn the full run into PASS.
- Fresh `cargo test --locked --test '*'`: host policy/restart, resource and service-log checks passed; telemetry remained **sandbox-blocked** at the same host-hold test (`sandbox_apply: Operation not permitted`).
- Fresh `cargo run --locked -- selftest`: passed 85 traces, formats, board read, terminal records and committed telemetry.
- `docs/shots/check.sh`, accepted image/source hashes and `git diff --check` passed. Previous JS evidence is reused because those source blobs and dependencies are unchanged.

The reviewer must assess the recorded full-run failure and green merge-result CI. No CI wait, merge or deployment is claimed.


## Merge-queue conflict repair

The merge queue removed PR #1319 for `merge_conflict`. Rebase onto `33b8da9892c2000af907ae1f39f55ff2e9f14ecd` reproduced a conflict in the manifest capture version, source commit and binary hash. Resolution retains the actual v4.6.23 capture provenance rather than relabeling accepted images as v4.6.24. The accepted README, images, manifest and snapshot sources are byte-identical to `98cfb8f80315480f0547fca188757fb4c4a2f9d3`; all manifest source/image hashes and `docs/shots/check.sh` pass. Rebase now completes and the base is an ancestor. No assertions or rules changed (R12, § 7.4).

Fresh macOS checks after rebase:

- `python3 scripts/check.py --base 33b8da9892c2000af907ae1f39f55ff2e9f14ecd`: failed in 59.947 s; 393 unit tests passed, 16 failed, 34 ignored. Inherited worker identity caused host-operation refusals in fixture tests.
- The same gate with `TASKQ_TASK`, `TASKQ_ATTEMPT` and `TASKQ_ROLE` removed only from test subprocesses failed in 26.307 s: 408 passed, 1 failed, 34 ignored. The previously recorded profile flush-error-prefix assertion still fails. No retry to green or protection change was made.
- `cargo +stable test --locked --test '*'` with the same isolated test identity: host policy/restart, resources and service-log integrations passed. Telemetry: 7 passed, 1 ignored, 1 sandbox-blocked (`sandbox_apply: Operation not permitted`).
- All 18 `tests/*.test.cjs` scripts, screenshot manifest regression checks, release-tag checks and exact-source release checks passed. Build and selector checks passed.
- Isolated offline selftest passed 85 golden traces, formats and committed telemetry; board is explicitly unknown in offline mode.

The full gate is not PASS. Green merge-result CI and reviewer assessment remain required; no merge or deployment is claimed.
