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

Owner visual acceptance under A4 remains pending. This evidence does not authorize merge or deployment.

## Visual inspection

The final native task list shows all three tasks and Home/Settings without the previous clipping. The workspace shows the complete request, reply, proposed target/effect and Confirm revision 2. Desktop (960 px) and narrow (390 px) page renderings loaded every image with no horizontal page overflow. Header and setup phrases remain intact; code blocks retain their own horizontal scrolling. At 390 px the workspace is an overview: open the linked original to read its text. This is a stated limitation, not mobile-native qualification.

- [Desktop queue](after-960-queue.png) and [exchange](after-960-exchange.png).
- [Narrow queue](after-390-queue.png) and [exchange](after-390-exchange.png).
- [Before desktop](before-960-queue.png) and [before narrow](before-390-queue.png).

`docs/shots/update.sh`, `docs/shots/check.sh`, manifest image/source hash verification, `sh -n docs/shots/update.sh` and all 18 `tests/*.test.cjs` scripts passed. No tests or assertions changed. The clipping failure and its replacement native capture above are the before/after proof for the snapshot-only repair (§ 7.4).

## Canonical checks

- `cargo test --locked` (worker identity variables removed only from test subprocesses): 409 unit tests passed, 34 ignored. Host policy, restart, resource and service-log integrations passed. Telemetry integration had 7 passed, 1 ignored and 1 **sandbox-blocked**: `resume_records_its_result_without_releasing_the_host_hold` reports `sandbox-exec: sandbox_apply: Operation not permitted`. The suite is not a full PASS; no permission change or retry bypass was attempted.
- `cargo run --locked -- selftest`: passed 85 golden traces, format checks, fresh board read, no overdue nonlive terminal defects and committed local telemetry.
- Green merge-result (`merge_group`) CI remains the publication gate under § 7. CI was not awaited. No merge or deployment occurred.
