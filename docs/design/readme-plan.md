# README page plan

Help a visitor install TaskQ, describe work, follow progress and answer a worker.
[Product](../../taskq.md#product) owns the page decisions (R13); use specq [Documentation](https://github.com/taskqorg/specq/blob/main/caveman/rules/documentation.md).

## Page contract

1. Keep `docs/header.webp` and `Agents working.` at the top.
2. Install has three labelled parts: (1) the exact existing-project setup phrase; (2) the exact repository plus explicit permission to use its Issues as the task board, with an existing-repository example and a new PRIVATE repository example naming its purpose in a disclosure; (3) one optional spec-driven development (SDD) note with a copyable English request: SDD is configured only when the user asks for it during onboarding, never by project enrollment alone. Close with setup confirmation: the agent shows account, repository, visibility and every external effect and applies them only after the user confirms; examples never replace that confirmation.
3. Give one purpose sentence, then three visual examples: describe work in Conversations, follow the queue, answer a worker. Use one coherent storefront scenario, one image and one short caption per example.
4. Put a few equivalent CLI commands in a disclosure. Keep one contract link, optional release checksum/provenance instructions, MIT and the final donate line.
5. Omit long dialogues, telemetry tables, runtime-switch logs and the second illustration. Keep deeper behavior in the contract and [use cases](../use-cases/README.md).

## Scenario and evidence boundary

The illustrative storefront change preserves a cart after sign-in. The owner drafts a request, the queue shows cart work and redirect review, then the cart worker asks how to merge duplicate quantities. The request screenshot shows a draft, not submitted work. The question screenshot preserves source choice numbers and recommendation. A screenshot does not prove tasks were created or an agent run completed.

Show shipped interface code, never design prototypes. Earlier [phrase evidence](readme-phrase-evidence.md) remains historical interpretation evidence. Do not advertise `Review #12.` as an automatic acceptance command. Setup, task admission and publication retain their approval boundaries.

## Images and acceptance

`docs/shots/update.sh [REF] [--bin PATH] [--publish]` captures REF (default `HEAD`) in a detached worktree, from one `--locked` build or from the release binary PATH once its `SHA256SUMS` entry, build provenance attestation and `--version` verify. It runs the real `taskq tray --snapshot` renderer in an isolated installation with no board token, service command or agent. `docs/shots/fixture.sh` supplies the queue; `tests/fixtures/conversation-shots.js` supplies Conversations request/question captures. Both use invented data and perform no board or agent operations. Keep the separate edge-case fixture unchanged.

Keep three dark images individually readable, with links to their originals. The manifest records the version, source commit, ref, binary origin and hash, environment with display scale, UI source blob IDs, capture-input hashes (fixtures, `update.sh`, `Cargo.lock`), image hashes and a reuse key over them; a matching key prints `fresh` without capture. Images, manifest and preview are staged and checked first, and `--publish` swaps images before the manifest; any failure keeps the previous set and prints one `update.sh: <stage>: <problem>; previous set kept` line. Preserve the summary icon in the manifest, but omit it from the page because its light glyphs disappear on light backgrounds. Capture runs on macOS only; Windows and Linux are unsupported. `docs/shots/check.test.sh` and `docs/shots/update.test.sh` cover the check verdicts and the failure, reuse and isolation paths.

Every run renders the README with the staged images (`scripts/readme-preview.cjs`) and checks it at 1280 and 390 px: every image loads, no horizontal overflow, disclosures open; `--publish` keeps the pages and `page-checks.json` in `docs/shots/evidence/<version>/`. Inspect the rendered README at desktop and narrow widths, including every image, setup disclosures and CLI examples. Preserve before/after renders and findings under `docs/shots/evidence/`. Source/image checks do not replace visual inspection or owner acceptance. Run `docs/shots/check.sh`, affected capture checks and the project-selected gate; report unavailable native or CI evidence explicitly.

## Contract detail

Linked from `taskq.md` [Product](../../taskq.md#product); this text binds like the contract (R8, R13).

### README rules

- Keep `docs/header.webp` and `Agents working.` first, then quick setup in three parts: install (exact phrase `Install taskq from https://github.com/taskqorg/taskq and set it up for this project.`); the exact repository with explicit permission to use its Issues as the task board (existing-repository example; new private repository for a named purpose in a disclosure); optional spec-driven development (SDD) by explicit onboarding request with a copyable one, never presenting enrollment as SDD setup; then the setup confirmation (taskq.md § 6), which examples never replace, one purpose sentence and three visual examples: describe work, follow the queue, answer a worker. Show shipped interfaces with coherent illustrative data, one screenshot and short caption each; a draft is not submitted work or completed-run evidence. Dark native captures use `docs/shots/update.sh` and its fixtures, pinned by `docs/shots/manifest.json`; owner acceptance needs before/after evidence ([UI acceptance](ui-acceptance.md#contract-detail)). CLI and release verification are disclosures; retain one taskq.md link, MIT, no badges/emoji clutter, and the final line `If taskq saves you time, [buy me a coffee](…).` Omit long dialogues, telemetry tables and runtime-switch logs.
