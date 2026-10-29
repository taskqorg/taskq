# Shared answer card

Accepted target under [taskq.md § 2](../../taskq.md#ask-card). TaskQ owns the component and its conformance fixtures; DashQ consumes it. This is not a shipped UI claim. Apply shared [operator rules](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md) and [AI-assisted controls](https://github.com/taskqorg/specq/blob/main/design/workflow.md#ai-assisted-controls).

Follow [shared-block inheritance B1–B4](shared-blocks.md). Compact placement does not authorize independent icons, tokens, counter layouts or simplified editors; the parent package governs visual fidelity.

## Ownership and delivery

One small browser-compatible component implements task question, source-numbered choices, recommendation, free text and receipt states. TaskQ workspace and DashQ supply adapters for their existing bridges; the component owns no credentials, network client, board parser or task state machine. No UI framework, service or general plugin system.

Keep source, styles/tokens and fixtures in TaskQ. Publish versioned static assets with the TaskQ release and checksum; until a v* release carries them, the pin is a TaskQ commit plus SHA-256 (§ Implementation, #937 owner answer). DashQ pins that version/hash and serves them locally under its CSP. Vendored generated assets are permitted only with provenance and checksum, never independent edits. No runtime CDN, iframe of the tray, npm registry or moving-main dependency. A compatible component update must pass both consumer fixtures.

## Card and interaction

- Consume TaskQ's canonical parsed card/snapshot: board identity, task identity/link, state, question/card revision, original text/evidence, ordered choices and recommendation. Parse with TaskQ's own semantics, never a second parser in DashQ. Unparseable cards remain readable with their board link and no write controls, except the explicitly enabled tray-only fallback below.
- Keep state, linked issue number and title one flowing block. Reveal the question with its task identity; each consumer places it (TaskQ target: the task's thread in workspace Conversations, [TW2](tray-workspace.md#contract-detail)). Preserve Done/Problem/evidence information; don't repeat boilerplate answering instructions when controls convey them.
- Text entry, attachments, validation and Send use the one [workspace composer](ai-workspace.md#native-dialogue-workspace-c1c9) (C3); technical operation and correlation IDs show only in details.
- Preserve choice IDs, order, text and recommendation. `★` has accessible name Recommended. Never sort the recommended choice first or renumber it.
- A normal choice submits that choice directly. A separate text field submits free text; the shared dispatcher wraps board lines under taskq.md § 5 without a reply line-count cap. Never infer a numbered choice from free text. No extra confirmation for an ordinary answer.
- Consequential operations, including a choice that TaskQ interprets as close/publication, stay visible in source order but unavailable in the dashboard with a reason and a board link. Do not silently remove or reinterpret them.
- Shared states: actionable, sending, accepted, stale, rejected, not sent (`failed`: nothing reached the board), partly applied (`partial`: a two-step action stopped after its first step), delivery unknown and read-only. Each receipt names its task and what it carries (`#N answer K. <choice>`, `text answer` or the fallback action). Show one local receipt beside the question; preserve row, scroll, focus and typed draft through refresh. Acceptance is one board operation, not worker resumption, task completion or repaired service.
- Bind a draft to board/task/question revision. A changed question disables the old submission; show the new question and retain the old draft without auto-submitting or silently rebinding it. Require an explicit review of the new question before another send.
- One operation ID binds one immutable answer payload and expected revision. Double click and uncertain-delivery reconciliation retain that ID; changed text/choice is a new operation. Disable duplicate controls while sending. Unknown delivery never becomes accepted on timeout or triggers a new-ID retry.

Tray-only fallback (R6, #1021): `capability.manage` enables requeue (nonempty reason, wrapped under taskq.md § 5), Later and confirmed Close only for `ask` with no canonical choices. Other consumers receive no new authority. Requeue sends `Reason: <literal reason>`; Close cancels without publication through `later` then `close`, with distinct stable operation IDs and the first readback revision fencing the second command. A failed/unknown step stops; a partial result names the parked task. Tray mounts cards only for ask, and retains operation receipts separately after a state change. Numbered close choices remain unavailable.

## Adapters and evidence

The adapter supplies answer capability/reason and invokes the canonical TaskQ dispatcher with actor, expected revision and operation ID. The component never emits board comments or decides authorization. An accepted receipt comes from TaskQ readback; failures retain producer reason/next action. Browser access and observation enrollment alone do not grant write permission.

Use the same fixtures in TaskQ and DashQ: choice and free text; long question; malformed card; review decision; read-only; duplicate click/lost response; stale revision; permission revoked; sending refresh; denied/unknown/accepted receipt. Check keyboard use, focus/drafts, 16/20 px, both themes and narrow layout. Pure UI tests cannot certify a real board write.

## Implementation

- Canonical fields: each `status --json` task row with a card adds `question`: `parsed`, original `text`, `done`, `problem`, `evidence` (label, URL), `choices` (`id`, `text`, `label` and `detail` split at its first ` - `, else the whole text and null, `recommended`, `consequential` for an option starting with `close`) and free-text `limits`. `parsed` is false unless the card has Problem and either 2-4 options with one `★` or no options; options must be numbered 1..n in order with no gap or repeat. A parsed card without options is an owner action (§ 2), answered with literal text. An unparsed card stays readable with its board link and no write controls. Outside `ask`, a row whose last question an accepted answer closed adds `answered {by, text}` (§ 5 late answer), so the tray, DashQ and PilotQ name that answer alike; a late answer through any surface, the CLI included, reads `already answered by <by>: <text>`.
- PM pick (taskq.md § 2): `taskq pick N.K --text R` writes `taskq: pick K` with the one-line reason as a board entry; no task state or revision changes. Fold keeps it while the same card is open; a newer card or any move out of `ask` (an owner answer included) clears it. The row adds `pick` = `{id, reason}`; status, inbox and this card show `PM N.K: <reason>` beside the worker's `★`, only the `★` without a pick. A pick never answers: choices and sends stay the owner's.
- Component: [src/answer-card.js](../../src/answer-card.js) (CommonJS source; `TaskqAnswerCard` browser global in the bundled release) and [src/answer-card.css](../../src/answer-card.css). `mount(el, adapter).update([{board, row}])` keeps drafts, focus and scroll and writes no DOM when the rendered card is unchanged; while the card is live, a disabled Send or Requeue states its free-text limit beside it, updated as the owner types (#1456); the adapter gives `capability(board, row)` = `{answer, reason, manage?, composer?}` (manage is tray-only; with `composer` the consumer's shared composer owns text entry, reading pure `compose(row, local, cap)` for draft, validation and Send label, and driving the card through the controller's `input(board, n, text, caret)` and `act(board, n, 'submit' | 'requeue')`) and `send(board, op)` resolving to `{state, reason, next}`, with `op` = `{id, task, revision, choice | text}`; tray fallback adds `action: requeue|later|close` and Close requires `confirmed: true`. Workspace adapters may also supply `load(board, row)` and `save(board, row, local)` for native draft, caret and operation-identity persistence. Restart restores pending delivery as unknown, never a replay. Enter uses the same submission validation as Send; Shift+Enter and IME composition do not submit. The mounted controller's `receiveReceipt(board, opId, reply)` applies a late receipt only to its matching operation, through the same receipt transition. A timeout can settle the original promise as unknown; a later exact receipt still updates that card without another send or promise settlement. Pure `status`, `step` and `render` hold all behavior.
- Workspace adapter: [src/workspace-answers.js](../../src/workspace-answers.js) embeds the shared assets in the exact task-answer context and keeps card instances through refresh/navigation. Menu task rows open that task thread without answering or acknowledging. The existing IPC bridge in [src/tray.rs](../../src/tray.rs) validates registered project/board identity and invokes the owner CLI dispatcher with the exact operation ID and revision; only its readback receipt accepts an answer. A missing receipt stays unknown and reconciles with the same payload/ID. A status refresh that reads the task past an answer operation's question (not a fallback action) reconciles once with the same payload/ID, so Sending ends with the CLI receipt; the refresh itself never accepts. Each Check delivery after an unknown receipt sends that op once more; `tests/tray-answer.browser.cjs` counts submit, reconcile and Check delivery as three sends of one op. `node tests/tray-answer.test.cjs` checks this adapter boundary. When another client answered first, the bridge's stale or rejected receipt carries `answered {by, text}` in place of revisions or CLI advice, and the refreshed row's `answered` names it even before this client sends: the thread names that answer, and while the worker runs it offers the unsent answer (late receipt or kept draft) as tray-only `action: steer` (#1785).
- Conformance: [tests/fixtures/answer-card.json](../../tests/fixtures/answer-card.json); `cargo test` checks its `question` fields against TaskQ's parse, and each consumer runs `replay(case, capability)` over it (`node tests/answer-card.test.cjs`). `tests/answer-card.browser.cjs` checks 16/20 px, both themes, 280/480 px, refresh and keyboard with Playwright.
- Release: `taskq-<version>-answer-card.js`, `.css` and `-fixtures.json` ship in each release, listed in `SHA256SUMS` with build provenance. DashQ pin (no v* release carries the assets yet, latest v4.1.11): vendor these three files from TaskQ commit `8715e1d2a63f725a290549234c720204d8234413` (#942, last change to all three) with that commit and these checksums as provenance; `git show <commit>:<file> | shasum -a 256` reproduces them. The historical pin above does not include the tray-only #1021 fallback. Checksums of the current source, checked by the node test:

`src/answer-card.js` SHA-256: `08ffeae55ea6becee25d789eeb6d6945f3a6cce5a3b8fbd5660c727cb7cbc29f`
`src/answer-card.css` SHA-256: `48f66124d87c0c4e69cf23ec1d4a760a1e560610d4d5639214ee650727c65314`
`tests/fixtures/answer-card.json` SHA-256: `a333ba192a5a6f628a38310e457f80524d0338d43c2dd201b55b78115efd5a69`

Parent source: TaskQ `531f68e2c6f971eb56c392e623e90721e674f346` supplies the JS/CSS/fixtures; design inheritance is pinned to `cf46e637d45f971dc83f1c1ea5f1e9a119a74256` (#943, K-A8 right alignment). The tray places the shared card beside its existing row without restyling it. This adapter fixes same-ID reconciliation when refresh removes an answered question; the updated JS/fixture hashes above identify that shared fix. Ask images (§ 2 visual evidence, #1782) render above the choices as thumbnails linking the full-size board URL; a thumbnail needs the `src` data URL the tray adds from the service's copy, else the image stays a link. Evidence links stay behind the Evidence disclosure ([One render](#one-render)).

B4 parent-to-child map, design `cf46e637`:

| Parent | Child | Placement differences | Evidence |
|---|---|---|---|
| [visual-style.md](visual-style.md) palette | `.qac` tokens and the tray body tokens | none | `tests/answer-card.parent.cjs` computed colors |
| `question()` in [plans-and-decisions.html](plans-and-decisions.html), `qcard` CSS of [workspace-prototype.html](workspace-prototype.html) | `render()` and `src/answer-card.css` | Head flows `? #N title` per M3; host/runtime stay on the tray row above; fixture state preview omitted; Done shows as its own line and option details below their buttons ([One render](#one-render), #1787) | divider, mark, choice fill/shape/numbers, wrap, Other answer and Evidence disclosures equal |
| Q1 states | `STATES` receipts | Recorded is "Answer accepted on the board" after readback | fixtures, `tests/tray-answer.browser.cjs` |
| K-A8 project strip | tray project header | unchanged tray counters | right edge, order, no overflow at counts 0/1/12/100 |

Deferred: Discuss with AI stays with the AI workspace route. The [#938 project-block extraction](project-blocks.md) fixes zero capsules (K-A2) and merged Doing/Review (K-A1); its release and native qualification remain separate. `PLAYWRIGHT_CHANNEL=chrome node tests/answer-card.parent.cjs` compares both themes, 16/20 px and 320/360 px; it is an instrument, not native qualification.

Delivery of #920 is the adapter PR first, per the owner's answer on the task. The commit/checksum pin above is #937's release evidence; native tray interaction is qualified on the next real ask card answered from the macOS tray (#937 owner answer), not on a synthetic task. Browser and dispatcher tests do not certify native interaction, a real board answer or worker resumption; README screenshot freshness remains a separate native capture gate.

## Accepted reference

[Interactive reference](answer-card-reference.html) uses the existing TaskQ design example, with synthetic task #841 and counts. It approves compact placement and ordinary choice/free-text/receipt interaction, not its mocked transport, invented state or approximate icons/tokens/counters. Shared-block B1–B4 takes precedence over those demo details. It does not replace the failure-state fixtures above.

Reference SHA-256: `4b857925b7a606f1766b84f2b0e79b36fb26bc1203d8a64a51ccdc29a321c556`.

## Contract detail

Moved verbatim from `taskq.md` (#1179) to keep the contract under its R8 size bound. `taskq.md` keeps the decision and links here; this text keeps the same authority (R8, R13).

Accepted shared UI target: [answer card](answer-card.md) owns the reusable TaskQ/DashQ presentation; § 2 and § 5 remain behavioral authority. Tray exception (R6): mount cards only in `ask` (target TW2: in the workspace, not the menu; semantics unchanged); without parsed choices, show text and owner requeue (reason), later and confirmed Close controls. Close cancels without publication through `later` then `close`, each with CLI readback; partial/unknown delivery stays explicit. An answer op is resent only with its own id: once when a refresh reads the task past its revision, once per Check delivery after an unknown receipt. Other consumers remain read-only for unparsed cards. K is an option number; a bare K or option text counts only in `ask` (section 2).

`--text`: literal Done/Problem lines; `--link`/`--image`: Evidence; `--option`/`--recommend`: options/star (moved from § 2, #1787).

### One render

Every surface that shows a parsed card (tray, workspace Conversations, DashQ) uses this one `render()` layout, in this order (#1787):

1. Head: the `?` mark, the task link `#N` and the title.
2. Done line after `✓` (accessible name Done), then Problem line after `⚑` (accessible name Problem), each in full.
3. Images ([Ask images](#ask-images), [Question types](#question-types)).
4. Options in source order. A button shows `N.` and the label, with `★` on the recommended one. The option's detail (`+ <gain>; - <cost>`) follows its button in full on its own line; it wraps and is never cut. A link in the detail opens inside the detail.
5. Other answer, PM pick and the receipt; evidence links behind an Evidence disclosure.

No surface shows the `To answer:` line: the controls convey answering, and the board card keeps the line for board readers. An option without ` - ` (a card written before #1787, or a service card such as retry/later/close) shows its whole text as the label and has no detail.

Option syntax: `--option "<label> - + <gain>; - <cost>"`. `ask` and `result` reject a missing label, a label over 32 characters, a label repeated in another option (case ignored) and a detail without one `+` gain and one `-` cost; the error names `option line K`. Limits count Unicode characters, so a Russian label of 32 letters fits.

### Question types

Each question type adds rules to [One render](#one-render).

- Visual choice: the options choose between things seen. Attach one `--image` per option, in option order. When the image count equals the option count (2-4), image K belongs to option K: `ask` names it `K. <name>` in the `Evidence:` line, and every app surface captions it `K.` below its thumbnail. Other image counts are plain visual evidence, without captions.

Browser delivery uses the self-contained `build/web/answer-card.js` bundle (`npm run build`), published under the existing release filename. Source checksums identify editable CommonJS modules; released bundle integrity comes from `SHA256SUMS`. See [JS packaging](profiling.md#js-execution-architecture).

### Human replies

Contract detail for taskq.md § 5. Every surface sends a person's answer, steer or requeue through the shared dispatcher wrapper. No input line, line-count or total-length cap applies. Store lines of at most 240 Unicode characters at word breaks; split an unbroken token only when necessary. Preserve every word in order and existing paragraph breaks. Refuse empty answers and steers with a fix. Any composer refusal shows the exact reason and fix beside Send and records its kind without reply content. Worker requeue verdicts retain the Reason/Next format in § 5.

A reply larger than one board comment uses ordered `taskq: reply-part <sha256> <index>/<count>` comments, each below 64 KiB, followed by one ordinary command entry referring to the parts. The digest covers the entire wrapped entry, including its command and operation fields. Parts are inert under § 2, including on N-1; only the final command moves the task. Read back every part before that command. A retry reuses matching parts; uncertain writes stop for readback. Readers assemble only trusted, unedited parts written before the command, in numbered order, and verify the digest and command fields; missing or changed data is unknown, never a truncated reply. The final entry retains its normal command/operation fields and tells N-1 readers to read the ordered comments through `inbox`, preserving the same fold without a new lifecycle transition. No local reply store is authoritative (R1).

The tray passes reply text in an owner-private temporary file to the bounded CLI process with `--text-file PATH`, keeping it out of process arguments and their OS length limit; it removes the file after the process ends. CLI and Telegram callers may use this flag or `--text-stdin` instead of `--text`; specifying multiple text sources is refused before a write. Both sources are UTF-8, with the same validation, operation ID, fencing and dispatcher as literal text. The file is transport, never queue truth (R1); draft persistence imposes no reply-length cap.

The shared workspace metadata budget excludes task-answer draft text, immutable operation text and the answered text/diagnostic in a late-answer receipt. Other metadata, attachment and owner-session draft limits remain in [C3](ai-workspace.md#native-dialogue-workspace-c1c9). Read and write apply the same budget so a large reply cannot make the workspace unreadable.

The tray also reads status JSON and command receipts through private temporary files: a large previous answer must not overflow the bounded child stdout buffer and block the next Send. `status`, `answer`, `steer`, `requeue`, `later`, `close`, `start` and `failed` accept `--json --json-file PATH`; the file contains the same JSON/JSON lines as stdout, without truncation. The tray removes its transport directory after the bounded child ends; it never treats those files as queue state.

### Ask images

An ask or result about something seen embeds its images in the § 2 `Evidence:` line; links alone do not count, and the board and the app show the images inline (the app as thumbnails that open full size). `ask` and `result` take `--image FILE` (PNG, JPEG, GIF or WebP, ≤ 10 MB), which uploads to the board before the card is written: GitLab project uploads embedded by absolute URL `https://<host>/-/project/<id>/uploads/<secret>/<file>#upload=<id>` (the relative or `full_path` link renders broken); GitHub one commit per image on the orphan branch `taskq-evidence`, embedded by commit-pinned `https://<host>/<repo>/raw/<sha>/<N>/<file>`. The board must serve the same bytes back with the token, else the command fails and writes nothing. The service copies each open ask's images to `.taskq/images` for the app, which holds no board token. `result --image` without a card adds one § 2 `Evidence:` line of images.
