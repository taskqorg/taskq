# Long conversation replies

Task: [#1816](https://github.com/taskqorg/taskq/issues/1816). Authority: § 5 human replies, R13 spec first, R12 evidence and § 7 delivery.

Answers, steers and owner requeue replies have no input length cap. One dispatcher wraps board lines at 240 Unicode characters and stores oversized replies in verified, ordered comments before posting the command. Private transport files preserve long input and JSON receipts outside process argument/output limits. Worker verdict, metadata, fencing and retry guards remain.

## Failure before and pass after

Before the fix, `node tests/answer-card.test.cjs` failed on the 267-character case with `At most 240 characters per line`. Before multipart transport, `cargo test --locked --bin taskq oversized_human_reply_is_complete_before_the_task_moves` failed with `answer: one oversized comment still reaches the board`. Those failures were recorded in the [earlier attempt](https://github.com/taskqorg/taskq/blob/1e6ae5543d9c35cdd83ee40434ced2083a913115/docs/design/review-evidence/1816/report.md).

Both regressions pass on the rebased candidate. Protection includes 267-character and 2000-character multiline replies, empty-input guidance, word order, Unicode, paragraphs, multipart completeness, N-1 folding, lost-response recovery, same-operation retries, stale-card races and reserved metadata. No assertion or timing deadline was weakened.

## Current candidate

Base: `2c20d3ce58a8b680ecc3754863bb80ed573a3fc8`. Tested source: `e84b41c2038a5592f5ac90f8feb367600d23d059`. Delivery changes only this evidence report afterward. Generated `docs/shots/*` and `*.sources.json` edits were dropped under § 7 step 4; release derives them. The amended contract rule is § 5 under R13.

Windows, stable GNU Rust toolchain, warm dependency cache, worker authority retained:

- `python3 scripts/check.py --base 2c20d3ce58a8b680ecc3754863bb80ed573a3fc8`: FAILED in 559.125 s, within the unchanged 900 s budget. Rust: 598 passed, 23 failed, 38 ignored. Setup, selected-gate checks and the rule-change gate passed. All six human-reply regressions passed in this run. The canonical gate is not green.
- Eighteen host-policy, installation, lifecycle, resource, tray-capacity and Windows security-restore failures are sandbox-blocked under worker authority. Diagnostics include `a worker cannot change host policy`, `a worker cannot install the host service`, `a worker cannot restart the host service or request Stop` and `Windows security restore requires an unrestricted operator`. Authority was not removed to run them.
- Three dialogue failures reported `program not found`: their fixture launches `sh`. With Git Bash tools on the process PATH, `cargo +stable test --locked --bin taskq dialogue::tests` passed 7 tests, with 1 ignored. The missing tool also affected the watch fixture's expected outcome.
- Two IPC fixture failures reported deadlines during the full run. `cargo +stable test --locked --bin taskq runtime::app_control::` passed 3 tests, with 1 ignored, including both failures. Assertions and deadlines are unchanged. This focused pass does not erase the full-run failures.
- `cargo +stable test --locked --test integration`: PASS, 17 passed, 1 ignored, 19.81 s in the harness.
- Every `node tests/*.test.cjs` file: PASS, all 26 files. The canonical runner stopped at Rust; these component checks ran separately.
- `node tests/tray-answer.browser.cjs`: PASS, all four light/dark and 16/20 px cases at 320/360 px. Playwright was restored as an unlisted test tool after `npm ci`; project dependency files are unchanged. Receipt delivery is substituted, not a live board write.
- Isolated offline `cargo +stable run --locked -- selftest`: PASS, 105 golden traces, formats and 9 committed capture observations. Board access is unknown/offline; the running host store was not used.
- `sh docs/shots/check.test.sh`, `sh .github/release-tag.test.sh` (Git Bash) and `python3 .github/release-check.test.py`: PASS.

The full gate remains failed. The focused passes and blocked-host classification do not qualify the full suite or native installation. Review and green merge-result CI (`merge_group`) remain required under § 7/R12.

## Composer evidence

1. [Before Send: 267 characters, Send enabled](01-before-send.png).
2. [After the matching success receipt](02-after-success.png).

Both images were regenerated from the rebased workspace bundle and visually inspected. Their SHA-256 digests exactly match the committed images: `a2bd5c3dc48c9093cc6685d4928ae036d870639a5c1bd6b16f4c83190d095a22` and `a96bcbd1460bf1a266570101b8d13c9abdb6641b4c75afc0c4d94407efea3724`. This two-screen set uses invented board data in Chromium headless shell on Windows and a substituted success receipt. Earlier native README captures are historical and are not shipped by this task.

Live Telegram, installed native Send and deployment remain unverified. No live host configuration or service was changed.
