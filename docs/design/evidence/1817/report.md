# Replies carry images and files

Task: [#1817](https://github.com/taskqorg/taskq/issues/1817). The [owner's answer](https://github.com/taskqorg/taskq/issues/1817#issuecomment-6497639463) bounds this work to TaskQ and a PilotQ-shaped fake. Live PilotQ integration is [#1857](https://github.com/taskqorg/taskq/issues/1857), held on `hermes-fra1`, depending on #1817.

## Candidate and scope

Canonical gate source: `02c2b261aeaef4c8c8e4f5c2572d248b17d7f16f`, rebased once on `20c8a6d37680098080717679d3ffe138d70055a0`. The final candidate changes only `taskq.md` and this evidence directory after that gate. Rust, JS, tests, dependencies and build scripts remain identical to that source. The contract follow-up shortens §5 without changing its decisions and passes the unchanged R8 guard.

R13/§5: task replies accept text, images and arbitrary regular files without TaskQ size/count caps. Owned private copies retain names and extensions; the CLI accepts repeated `--file` or `--files-from` JSON path lists. Existing board adapters upload and read back every file before the command comment. Accepted board manifests reconstruct verified worker files before native submission. Source files, operator grants and owner-session C3 limits remain protected.

The rebase preserves #1838: files accompany a changed/late answer's durable note, retaining its original question revision. Model acceptance determines which comments may supply files. The current question, retry identity and unknown-delivery fence remain intact.

## Before and after

Before implementation, `node tests/reply-files.test.cjs` failed with `file-only image + 20 MiB document must reach the operation`: the actual operation was `undefined`. After implementation the same test passes, including mixed files, file-only input, pending/refused copies, unknown retry, changed questions and more than four files.

The first real HTTP fixture failed with Windows socket error 10035: an accepted socket inherited nonblocking mode. Setting that fixture socket to blocking retains every request/body assertion; the 20 MiB POST, response and download now pass. No production socket policy changed.

The first canonical run found the contract at 61,866 bytes, over R8's unchanged 61,440-byte ceiling. After condensing §5, the contract is 61,439 bytes and 295 lines; `core::tests::every_table_row_has_a_golden_trace` passes. No trace, limit or assertion was removed.

## Verification

Windows; Node 24.5.0, Python 3.12.10, Playwright 1.64.1. Existing Git Bash tools were added only to the test process's PATH. No host policy, installation, service or live Telegram client was changed.

| Check | Observation |
|---|---|
| `python3 scripts/check.py --base 20c8a6d37680098080717679d3ffe138d70055a0` | Full selection; 615 passed, 19 failed, 38 ignored; 279.266 s. Failed, never reported green. |
| Attachment checks in that full run | All five pass: downloaded media/worker copies, private storage/refusal, real HTTP, CLI input and held worker submission. Includes a valid PNG, a 20 MiB file, Unicode names, preserved binary bytes, corruption and same-operation retry. |
| Remaining full-run failures | Eighteen host-policy/install/lifecycle/resource/security fixtures under worker authority: sandbox-blocked (§7.4). Observed refusals include `a worker cannot change host policy`, `a worker cannot install the host service`, `a worker cannot restart the host service or request Stop`, and `Windows security restore requires an unrestricted operator`. No permission guard was bypassed. |
| R8 follow-up | The unchanged golden-table/size check passes after the contract-only correction; full Rust was not rerun for this prose-only correction. |
| `node tests/*.test.cjs` via a PowerShell file loop | All 27 files pass. Source/asset provenance and existing answer/steer controls remain checked. |
| `node tests/reply-files.browser.cjs` | Both Conversations and the tray-to-Conversations route pass paste/drop/pick, mixed text/files, image + 20 MiB, copy identities, exact refusal/fix and refusal telemetry without reply content or file names. Native storage/board receipt is an explicit fake. |
| Existing `tests/tray-answer.browser.cjs` | Passed all four light/dark and 16/20 px journeys during development. The combined post-rebase Node fixtures also pass #1838's changed-question semantics. |
| Isolated offline `cargo +stable run --locked -- selftest` | 107 golden traces and formats pass; nine capture observations commit in the temporary store. Board is unknown/offline, as expected. |
| `cargo +stable test --locked --test integration` | 17 passed, one ignored. |

The canonical run stopped at Rust failures. Node, offline selftest and integration were therefore run separately; their passes do not convert that run to green. Merge-result CI and target-platform acceptance remain separate under R12/§7.

## Four-screen review set

Chromium captures of the published HTML bundle, dark theme, 900 × 850, with invented project/task/file data. The tray's actual Open question action is checked before loading its shared composer with a substituted native bridge. These are browser observations, not installed native-window or live bot acceptance. Both entry routes reach the same composer, so their before views are identical.

| Entry | Before Send | Refused Send, files kept |
|---|---|---|
| Conversations | [Before](workspace-before.png) | [Refusal](workspace-refusal.png) |
| Tray | [Before](tray-before.png) | [Refusal](tray-refusal.png) |

![Image and 20 MiB file beside Send](workspace-before.png)

![Exact refusal and fix beside Send; draft and files retained](workspace-refusal.png)

Not deployed. Live PilotQ receipt is unverified here and belongs to #1857. Public GitHub publication still requires green `merge_group` CI; this worker does not wait for CI (§7).
