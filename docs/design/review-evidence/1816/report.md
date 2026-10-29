# Long conversation replies

Task: [#1816](https://github.com/taskqorg/taskq/issues/1816). Authority: § 5 (human replies), R13 (spec first), R12 (separate evidence) and § 7 (result candidate).

Long answers, steers and owner requeue replies have no input length cap. The shared dispatcher wraps board lines at 240 Unicode characters and splits oversized replies into verified, ordered board comments before posting the command. Native transport files avoid process argument and output limits. Draft persistence exempts reply text from metadata budgets; unrelated metadata bounds remain.

## Failure before and pass after

Before the implementation change, `node tests/answer-card.test.cjs` failed on the new 267-character case: expected no problem, received `At most 240 characters per line`. The same component check now passes, including a 2000-character multiline reply and empty-input guidance.

Before the oversized transport change, `cargo test --locked --bin taskq oversized_human_reply_is_complete_before_the_task_moves` failed with `answer: one oversized comment still reaches the board`. Its regression now passes with word order, Unicode, paragraph breaks, complete readback before transition, N-1 fold equality, lost-response recovery and same-operation deduplication. Changed, missing, untrusted or edited parts remain unknown.

Choice, stale-card, draft preservation, cancellation, worker fencing, operation identity, reserved metadata and worker Reason/Next validation remain protected. Only the accepted § 5 human-reply length decision changed. Test assertions and timing deadlines were not weakened.

## Earlier-attempt evidence (historical)

The implementation is rebased onto `5d3540af629c51138b2a561c4511c495719c2b06`. Main's `--transcript`, `later --until`, transcript formatting, credential masking and service-delivered author attribution remain. A range comparison confirms the implementation and regression-test patches are unchanged through the final rebase; only evidence conflicts needed resolution.

Code candidate: `37bd078baf6c1f4f04c959c654d4af037baada07`. The delivery changes only evidence and provenance after this source was tested. Its product, test, dependency and contract blobs are checked unchanged before handoff (R12).

`python3 scripts/check.py --base 5d3540af629c51138b2a561c4511c495719c2b06` timed out after 900.141 s on Windows during Rust compilation, before the test harness started. Worker authority and the 900 s budget were retained. The canonical gate is not green; its setup and rule checks do not establish a full Rust-suite pass.

The earlier source `1bf7c4a37e6208e96408853958b42716c872af67`, on base `fdc61b84`, completed its canonical gate in 651.015 s: 594 passed, 20 failed and 38 ignored. Eighteen host-policy, installation, lifecycle, resource and security-restore failures were sandbox-blocked under worker authority; the other two passed individually with unchanged assertions. That failed earlier-source run is separate from the final source's evidence and is not claimed as a pass.

The remaining checks completed separately on the final source:

- `cargo +stable test --locked --bin taskq human_repl`: all six human-reply regressions pass, covering wrapping and word order, multipart readback, concurrent card replacement, lost-response recovery, stdin and large JSON receipts. The additional `reply` filter passed seven cases with two native probes ignored.
- `cargo +stable test --locked --test integration`: 17 passed, none failed and one ignored; 17.83 s in the harness.
- Every `node tests/*.test.cjs` file: all 26 pass, including provenance and architecture guards.
- `node tests/tray-answer.browser.cjs`: all four light/dark, 16/20 px cases pass at 320/360 px. Long answer, multiline requeue, long steer, empty input, exact refusal/fix, telemetry and draft preservation are covered. The two composer screens were refreshed.
- Isolated offline `cargo +stable run --locked -- selftest`: 105 golden traces, formats and nine committed capture observations pass. Board access remains unknown/offline.
- Screenshot-provenance, release-tag and exact-source release-check instruments pass. The unchanged artifact scripts reuse their checks from the preceding base. The README capture uses the fixture/capture/manifest pipeline from `docs/shots/update.sh`, with a scratch debug-build variant that reuses the tested binary. The manifest names the debug renderer and pins this source, the binary, UI blobs and all image digests; explicit verification passes.

## Earlier-attempt screens (historical)

The two composer screens use invented board data in the actual workspace bundle, Chromium headless shell on Windows. The success receipt is substituted; this is browser/IPC evidence, not a live board write.

1. [Before Send: 267 characters, enabled Send](01-before-send.png).
2. [After the matching success receipt](02-after-success.png).
3. [Native project route](../../../shots/popover-dark-project-16.png).
4. [Native icon route](../../../shots/icon-summary-run-dark.png).

The native README captures use Windows WebView2 and `docs/shots/fixture.sh` in an isolated temporary installation, refreshed immediately before capture. They were rendered with `cargo +stable run --locked -- tray --snapshot DIR --route project,icon` from the tested source. `docs/shots/manifest.json` binds that source's UI blobs, the exact debug binary digest and the image bytes. All four screens were refreshed and visually inspected on this source. Explicit digest verification passed for the binary, all UI Git blobs and every manifest image; the screenshot guard alone does not verify image bytes.

Live Telegram transport, installed native Send clicks and deployment were not exercised. Review and green `merge_group` CI remain required for publication under § 7/R12; these source, browser and native captures do not replace that gate.

## Current continuation

Rebased onto `2c20d3ce58a8b680ecc3754863bb80ed573a3fc8` under § 7. The earlier task commits remain in a preservation ref; this branch contains one implementation commit. Generated `docs/shots/*` and `*.sources.json` edits were dropped as required by § 7 step 4. The earlier screenshots and test observations above are historical, not acceptance of the rebased candidate (R12).

The accepted human-reply decision changes § 5 under R13. No other rule changes are intended. The current candidate's canonical gate and affected browser checks are recorded below after completion. Native installed Send, live Telegram and deployment remain unverified.
