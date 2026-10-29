# Closed pins and archived task entry · #1829

The candidate applies R1/R6/R12/R13 and § 7. It amends Product, § 5 and P-W1 for the requested closed-pin lifetime and exact read-only lookup. Existing geometry, board authority and admission stay intact. The source hashes and renderer are pinned in [manifest.json](manifest.json).

## Failure and replacement evidence

- Before: `node tests/closed-pins.browser.cjs` expected three exact issue requests outside Completed; the baseline sent zero (`0 !== 3`).
- Before: the same check with baseline `src/workspace.js` selected Recent for a locally archived task (`aria-pressed: false`, expected `true`). The candidate source was restored after this isolated reproduction.
- After: the same check passes exact GitHub/GitLab pin reads, sourced outcome icons/verdicts, unknown with Retry, closed action targets, read-only transcript, first-open and 24-hour expiry, manual Unpin, retained Completed rows and both local/event archive entry paths.
- Retained protection: `tests/task-dashboard.browser.cjs` and `tests/conversations-selection.browser.cjs` pass the neighboring view/filter/draft/selection routes. `tests/task-views.test.cjs` retains only the pin reference and closure-observation preference across normalization.
- Rust: `cargo test --locked --bin taskq closed_pin` passes the exact outcome and host fence plus GitHub/GitLab closure parsing. The existing Completed projection and merge-receipt checks remain unchanged.

## Review set

| Acceptance | Status | Evidence |
|---|---|---|
| A1 | Pass, model/adapter/browser | [Closed pins and unreadable board](01-closed-pins-and-unreadable.png); exact issue outside page one, GitLab without a close reason, explicit read failure |
| A2 | Pass, model/browser | [Native transcript, read-only](02-read-only-transcript.png); issue/PR targets, no continuation command or composer; remote transcript gives a reason |
| A3 | Pass, browser | [Completed after unpin](04-completed-after-unpin.png); virtual 24-hour boundary, explicit transcript/issue open, manual Unpin, retained history |
| A4 | Pass, browser | [Menu entry into Archive](03-archived-menu-entry.png); both local archive reference and archived question event select Archive, with Restore and no answer/steer controls |
| A5 | Pass, scoped regression | One four-screen set; affected checks above. These images show bundled production components with invented receipts. |
| Native / Windows | Not tested | Browser/adapter evidence does not qualify installed behavior. Windows native transcript lookup retains its explicit unsupported reason. |

The four images were inspected at their natural sizes. The only visual differences are sourced closed-pin metadata/actions and a read-only archived task with direct Restore. The Completed-row grid now wins the menu task-row rule, keeping the outcome icon beside its title.

## Delivery gates

- Before: the rule gate refused Product and §5 because the task omitted their IDs. The [owner answer](https://github.com/taskqorg/taskq/issues/1829#issuecomment-6494020587) authorizes both; the gate now passes (R13).
- The first full run exposed this candidate's R8 defect: `taskq.md` was 61,910 bytes, above 61,440. The root now records the pin lifetime and links P-W1 for detail; Product/§5 prose is shorter with the same requirements. At 61,433 bytes, the unchanged `core::tests::every_table_row_has_a_golden_trace` passes, focused and in the final full run. No guarding assertion changed.
- `python3 scripts/check.py --base 93877cc0ced95a0858dc72d829ba51337f33df59` on source candidate `71c7aa98` passed selector checks, rule authorization and the web build. It failed in Rust: **593 passed, 18 failed, 33 ignored**, 72.721 s on the warm macOS worker. Every closed-pin, Completed and exact history-read check passed.
- Host policy/install/Stop checks reported `a worker cannot change host policy`, `a worker cannot install the host service` and `a worker cannot restart the host service or request Stop`: **sandbox-blocked**, not PASS. The remaining assertion-only host/control failures, including selected recovery admission, remain **unknown**; their causes were not established. Worker authority was not removed or widened. The full run stopped before integration, offline selftest, Node and artifact-script stages. The affected Node/browser checks were run separately and pass; the neighboring browser evidence above remains valid for unchanged sources. Merge-result CI remains required (R12, § 7).
- `docs/shots/update.sh` passed on macOS 27.0.1 arm64: an isolated fixture installation captured project/icon routes in WKWebView from source `761de66d02ff1ed86f971f8c33f8babce80988c1`; the release build took 6m 23s. The project image is byte-identical to the prior README image. The generated Summary icon and UI-source blobs are pinned in `docs/shots/manifest.json`; `docs/shots/check.sh` passes. Both artifacts were inspected. These native captures qualify only the README fixture routes, not closed-pin/archive scenarios.

This package is implementation evidence, not merge, deployment or native qualification proof.

## Rebase correction

- Rebased onto `bbeeae9af4acff3a4148def94e054ce5cf80f32c`; the only conflict was the generated README screenshot manifest. It is regenerated below rather than combining receipts from different builds.
- Before: `taskq.md` was 61,448 bytes after rebase; `cargo test --locked --bin taskq core::tests::every_table_row_has_a_golden_trace` failed with `taskq.md over 60 KiB`. After: 61,433 bytes and the same guard passes. The README section keeps the requested `what you get` name. No rule or guarding assertion was weakened (R8/R13).
- `npm run build` and `node tests/closed-pins.browser.cjs` pass on the rebased source. All four regenerated acceptance images are byte-identical and were inspected at natural size; the source manifest now names the rebased inputs. Task-view normalization, neighboring dashboard and Conversations selection checks pass again.
- `docs/shots/update.sh` passes from source `226149b818fad306d7e25cef40e46a7fefd4c5b5` (release build: 2m 43s), and `sh docs/shots/check.sh` passes. The project capture is unchanged; the regenerated Summary icon and both images were inspected. The manifest records this build's UI blobs and binary checksum, without conflict markers or mixed receipts.
- `python3 scripts/check.py --base bbeeae9af4acff3a4148def94e054ce5cf80f32c` on that source fails: **594 passed, 18 failed, 33 ignored**, 123.354 s total. The selector/rule gate, web build, R8 guard and closed-pin Rust checks pass. The same host/control failure set remains: explicit worker refusals are **sandbox-blocked**, and the remaining assertion-only failures are **unknown**. No worker authority or test assertion changed. Integration, offline selftest and the remaining canonical Node/script stages did not run; the affected browser/component checks above passed separately. Acceptance still requires green `merge_group` CI (R12, § 7).
