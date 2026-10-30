# README page plan

Help a visitor install TaskQ, describe work, follow progress and answer a worker.
[Product](../../taskq.md#product) owns the page decisions (R13); use specq [Documentation](https://github.com/taskqorg/specq/blob/main/caveman/rules/documentation.md).

## Page contract

1. Keep `docs/header.webp` and `Agents working.` at the top.
2. Lead with the exact existing-project setup phrase. Explain setup confirmation; put the new-project variant in a disclosure.
3. Give one purpose sentence, then three visual examples: describe work in Conversations, follow the queue, answer a worker. Use one coherent storefront scenario, one image and one short caption per example.
4. Put a few equivalent CLI commands in a disclosure. Keep one contract link, optional release checksum/provenance instructions, MIT and the final donate line.
5. Omit long dialogues, telemetry tables, runtime-switch logs and the second illustration. Keep deeper behavior in the contract and [use cases](../use-cases/README.md).

## Scenario and evidence boundary

The illustrative storefront change preserves a cart after sign-in. The owner drafts a request, the queue shows cart work and redirect review, then the cart worker asks how to merge duplicate quantities. The request screenshot shows a draft, not submitted work. The question screenshot preserves source choice numbers and recommendation. A screenshot does not prove tasks were created or an agent run completed.

Show shipped interface code, never design prototypes. Earlier [phrase evidence](readme-phrase-evidence.md) remains historical interpretation evidence. Do not advertise `Review #12.` as an automatic acceptance command. Setup, task admission and publication retain their approval boundaries.

## Images and acceptance

`docs/shots/update.sh` builds with `--locked` and runs the real `taskq tray --snapshot` renderer in an isolated installation. `docs/shots/fixture.sh` supplies the queue; `tests/fixtures/conversation-shots.js` supplies Conversations request/question captures. Both use invented data and perform no board or agent operations. Keep the separate edge-case fixture unchanged.

Keep three dark images individually readable, with links to their originals. The manifest records the version, source commit, binary hash, environment, UI source blob IDs, capture-input hashes and image hashes. UI sources must be committed before capture; capture-input hashes identify fixture/script changes independently. Preserve the summary icon in the manifest, but omit it from the page because its light glyphs disappear on light backgrounds. Windows capture remains unqualified.

Inspect the rendered README at desktop and narrow widths, including every image, setup disclosures and CLI examples. Preserve before/after renders and findings under `docs/shots/evidence/`. Source/image checks do not replace visual inspection or owner acceptance. Run `docs/shots/check.sh`, affected capture checks and the project-selected gate; report unavailable native or CI evidence explicitly.
