# One-action answer flow evidence

Baseline: `c761cc2ed608b26638bc295dc5f3e5a6690ffb68`. Applies R1, R8, R12, R13 and sections 2, 5, 7. Owner choice: late answers steer running work; acceptance stays binding. The task records the Codex proposal, the independent Claude Opus 5.5 proposal and the owner answer.

## Failure before and pass after

A development probe calls the shared card's `step` with the existing stale-revision fixture: type a draft on revision 4, refresh to revision 6, submit once; separately click Close once on a no-choice ask.

Before: ordinary Send emitted an operation, changed-question Send returned null (`stale`), and Close returned null with `confirmClose: true`. The existing card suite passed because it enforced those blocking rules.

After: ordinary Send still emits once; changed-question Send emits the retained text at revision 4 for the dispatcher to save as a note; Close emits `action: close` immediately with no confirmation. The permanent fixture covers the new question staying open and the note receipt; the dispatcher check covers reused option numbers, races, wrong revisions for other actions and same-ID replay. Golden traces protect note order, replacement, duplicate/edited/fenced inputs and an unchanged newer ask.

The rule gate previously ignored the context of a picked ask, requiring the owner to restate `Product` and other rule IDs. The `task_text` boundary regression failed before and passes after: it includes the selected ask context and option, while preserving trust, revision, attempt, duplicate-operation and unchosen-option fences. Unsupported lifecycle histories grant no inferred approval. Readback through #1805 answer `6490154306` confirms that its picked ask contributes `Product` before the later answer `6491130511` restated that ID.

Review also reproduced a raced Later choice returning stale without a note and an exact-SHA approval remaining actionable in the tray. The added `review_` regressions fail on both before the fixes; their candidate outcomes are recorded with the Rust gate. These protections retain cancellation/publication fences while preserving late intent.

## Browser evidence

Synthetic fixtures, Chromium headless on Windows; these are component/adapter observations, not native tray, real board or worker-delivery proof. Three screens, below the task's six-screen cap:

1. [Updated question and retained draft](01-changed-question-keeps-send.png): Send stays available; the inline status names the note destination.
2. [Accepted task note](02-note-receipt.png): board acceptance is distinct from task completion or worker delivery.
3. [One explicit cancellation action](03-direct-cancellation.png): no intermediate confirmation screen.

`node tests/answer-card.test.cjs` and `node tests/tray-answer.test.cjs` pass. The shared-card browser instrument passes both themes, 16/20 px and 280/480 px. The workspace bridge instrument passes both themes, 16/20 px and 320/360 px: draft, caret, focus, literal text, changed questions, first-action Close, duplicate clicks, same-ID reconciliation and late receipts.

The confirmation inventory and compatibility boundaries live in [the answer-card contract](../../answer-card.md#confirmation-inventory). Goal, acceptance and every ordered board note are included in attempt/resume prompts and fresh inbox hand-in instructions; workers and reviewers must ask immediately on conflicting notes. This is prompt/context enforcement, not a claim that code can prove a model's semantic review.

The `docs/shots/fixture.sh` native refresh also produced the README project and icon images using isolated Windows WebView2 from UI source commit `54ee4b2e5a096064b095f4a9a77d90189d4b6ccc`; the completed GNU binary was reused via Windows CreateProcess with its build-provided WebView2 loader, both images were inspected, and their exact hashes are in `docs/shots/manifest.json`. These two existing README routes plus the three flow screens keep the delivered set to five. Native project/icon capture does not qualify real answer delivery.

Rust and canonical merge-candidate checks are recorded on the task and PR with the exact candidate. Publication requires green merge-result CI; no production install is part of this change.

## Rebased candidate

The candidate includes `2c20d3ce58a8b680ecc3754863bb80ed573a3fc8`, retaining service-delivered turn attribution as well as the earlier menu and timed-parking fixes. Conflict resolution preserves main's archived-thread behavior, timed-parking contract and all six timed-parking golden traces alongside the durable-note traces. The original publication requirements and the R8 contract-size guard remain unchanged.

The three flow screens above were regenerated from the combined frontend. The [project capture](04-project-native.png) and [icon capture](05-icon-native.png) use the same isolated screenshot fixture; [the capture record](native-capture.json) records the captured source, binary and UI-source hashes. The native artifact predates the last rebase; every recorded UI blob is unchanged. The owner-approved ?7 rule from #1841 keeps this evidence in the task directory, with shared shots and design-hash lists inherited from main. These five screens remain within A3's six-screen limit. Native snapshots qualify rendering on Windows WebView2, not an installed service or live worker delivery.

The Windows candidate gate exposed an obsolete Steer assertion and a native-pipe fixture accept race. Both focused checks pass after replacing that assertion with saved-note/unchanged-state checks and adding a peer readiness handshake; all publication and I/O-deadline assertions remain. Eighteen operator/host tests refused the inherited worker identity and remain `sandbox-blocked`, not passed (R12).

Rebasing also invalidated shared consumer source hashes for the changed answer modules. The provenance assertions failed before their exact SHA-256 entries were refreshed; the assertions remain unchanged.

## Current conflict repair

Rebased onto `9b342b9c6992a8144fcdb1d593f66bbacbb47d9f`. Before: Git refused the rebase in the answer-card contract, workspace receipt adapter, root contract and tray browser instrument. After resolution: the browser instrument passes both themes and both text sizes, sending a 267-character draft against its original question revision without a second confirmation. The merged flow preserves #1816's uncapped human replies, refusal telemetry and inline reason/fix. Existing capture files above remain historical observations; the native UI hashes are not a qualification of this combined frontend. No new captures were required for conflict repair.

The source-checksum assertion failed because it required generated hashes in the shared design page. Under the existing section 7/#1841 rule, answer-card now uses the existing build-derived manifest mechanism. Its regression still checks every pinned source digest and additionally checks that release staging includes the bundle, stylesheet, fixtures and derived manifest. No provenance protection was removed.

The combined candidate's first canonical gate exposed three further conflicts: the contract exceeded R8 by 10 bytes; the multipart race check expected an owner steer to be rejected; and the large-JSON receipt check expected a late answer refusal. The contract wording was shortened without removing a rule. The stale multipart cleanup scenario now uses requeue, which still requires a current revision, retaining bounded-edit and newer-card preservation assertions. The large-JSON scenario now expects the approved saved-note receipt, retaining the full winning answer and adding exact durable-note readback. The worker/host restriction failures remain separate and unqualified.
