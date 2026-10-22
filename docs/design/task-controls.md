# Shared task controls

Accepted under [taskq.md § 5](../../taskq.md#5-commands) (#1506). TaskQ owns the control table, component and fixtures; DashQ's Tasks view ([dashq#170](https://github.com/taskqorg/dashq/issues/170)) and the tray consume them. § 2 transitions and § 5 receipts stay the behavioral authority.

## Contract

Version `taskq.task-controls/1`. Each `status --json` / `status.json` task row lists `controls`, the actions its state offers:

| Action | CLI command | Fixed text | Offered in |
|---|---|---|---|
| `start` | `start N` | none | `held` |
| `stop` | `failed N` | `Reason: Stopped by the owner.` | `ready`, `waiting`, `doing`, `review`, `ask` |
| `later` | `later N` | `Reason: Parked by the owner.` | `ready`, `waiting`, `doing`, `review`, `ask` |
| `retry` | `requeue N` | `Reason: Retried by the owner.` | `later`, `failed` |

Every command adds `--json --op-id <id> --revision <row revision>`. The receipt is § 5's `{task, op_id, outcome, state, revision, reason, next}`: another revision is refused `stale`; a seen op id returns the first receipt and writes nothing, as in the tray. An op carries no free text or choice; the fixed text is the whole reason. Stop holds the task as `failed`; the service stops a running child.

Close is not a control. It is consequential (cancellation or publication), like a close choice on the [answer card](answer-card.md): consumers show the task's board link (`Close on board ↗`) instead. Ordinary answers keep the answer card's gates; resource controls keep [theirs](shared-resource-controls.md).

## Component and fixtures

- [src/task-controls.js](../../src/task-controls.js) (browser global `TaskqTaskControls`): `render(row, {key, url, except})` emits one button per listed control (`data-a="control"`, `data-control`, `data-key`) and, with `url`, the board link; `op(row, action, prefix)` returns `{id: prefix.action.n.revision, task, revision, action}`. The consumer's adapter sends op through its TaskQ dispatcher and shows the receipt; the component holds no credentials, network client or state machine.
- Tray (owner decision 2026-10-22, #1523): task rows carry no repeated action buttons. One `⋯` menu button at the end of each row's top line opens that row's menu: Answer (for `ask`), Start (for `held`), then Stop, Later and Retry as the row's `controls` list them. The button exposes `aria-expanded`, `aria-controls` and the task number; Escape closes the menu and returns focus to the button. Batch Start of several held rows stays in the group header; `src/tray.rs` maps op to the table above, checks the row offers it at the same revision, and runs the owner CLI. Receipts reuse the answer notices: Sending, accepted, stale, rejected, unknown; Check delivery resends the same op id.
- [tests/fixtures/task-controls.json](../../tests/fixtures/task-controls.json): version, table, offered controls per state, op-to-argv cases and receipt cases. `cargo test` checks the Rust table, row offers and tray argv against it; `node tests/task-controls.test.cjs` checks the component and bundle.
- Release: `taskq-<version>-task-controls.js` and `taskq-<version>-task-controls-fixtures.json` ship in each release, listed in `SHA256SUMS` with build provenance. DashQ pins one release version and its checksums, never moving main.
