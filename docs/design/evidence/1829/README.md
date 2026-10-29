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

`python3 scripts/check.py` after rebase selected the full gate, passed whitespace and selector self-checks, then stopped: `rule gate: the task does not name Product (changed), §5 (changed)`. No later full-gate steps ran. The task describes the requested behavior but does not literally name these IDs; the recorded rule-change gate requires an owner answer before acceptance. `docs/shots/update.sh` passed on macOS 27.0.1 arm64: an isolated fixture installation captured the project/icon routes in WKWebView from source `761de66d02ff1ed86f971f8c33f8babce80988c1`; the release build took 6m 23s. The project image is byte-identical to the prior README image; the generated Summary icon and all UI-source blobs are pinned in `docs/shots/manifest.json`. Both artifacts were inspected. These native captures qualify only the unchanged README fixture routes, not the closed-pin/archive scenarios. This package is implementation evidence, not merge, deployment or native qualification proof.
