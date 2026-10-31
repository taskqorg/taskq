# Status report

The human report and the status file that every surface reads.

## Contract detail

Linked from `taskq.md` [R6](../../taskq.md#r6-human-report) and [§ 4](../../taskq.md#4-service); this text binds like the contract (R8, R13).

### Report parts

Tray/browser project summaries and task rows (M3) reuse [shared project blocks](project-blocks.md), with separate Doing/Review, [counters/filters](task-counters.md) with Held and no Closed counter:

1. `<project> · [board](<url>)`, preceded while a local `status.json` within `ttl` reports a board rate-limit pause (§ 4) by `Rate limit: board requests paused until <time> (<host>)`, else by the § 4 low-budget line, also when this read fails.
2. `In work N · Waiting for answer N · Ready N`. In work: `doing` + `review`. Waiting: `ask`. Ready: `ready`.
3. `| Task | State | Runtime | Session |`, a row per open task except `held`, `ask`, `later`, `failed`, by priority, number; task cells link; sessions link Claude's Remote Control URL, Codex `codex://threads/<id>` on its own host only, elsewhere the DashQ progress page, else plain host; `waiting` rows say `blocked ([#M](url) open)`: each § 1 dependency linked, `open` or `unknown: <reason>`; `--json` rows add `blocked` (`ref`, `url`, `reason`), ready/waiting rows and reviews awaiting a reviewer `admission` ([codes](enrollment.md#contract-detail)); a `capacity` code reads `Waiting for a free slot · <runtime> slots full`; `host-<name>` without a heartbeat, `No host <name>`. Staged tasks (§ 2) show `stage k/n` in State, `Later:`, tray.
4. `Questions (answer N.M):` and `| Question | Brief reason | Options |` with `N.1 <option> · N.2 <option> ★` (★ recommended), a row per `ask` and `review` with options; an owner action (§ 2) reads `waiting for owner: <action>`. Then [`Decide:`](reports.md#decide) rows.
5. `Held: [#N title](url), ...`, then `Later:` (timed: `until <local time>: <reason>`), one line each; then a `Failed:` reason table; then `Stuck: [#N title](url) (<state>, <age>), ...`: last log entry ≥ 2 h old in `held`, `ask`, `later`, unleased `doing`/`review` (§ 3), or at once `waiting` with every blocker closed; `--json` rows add `stuck` true and `age_s`. Observe only.
6. Tray running agents follow [M3](menu.md#running-agents) (target: Home panels, TW3). `Hosts: <host> v<version> <age>[ stale][ focus off|unknown][ <rt> limit until|probe <time>][ tray <code>], ...` from heartbeats (§ 4).
7. `Next: 1 <action> ★ · 2 <action>` from the PM ([shared Next contract](https://github.com/taskqorg/specq/blob/main/design/workflow.md#ai-assisted-controls)).

### Status file and events

- Each pass writes `<root>/.taskq/status.json` for `service status`, tray and DashQ: coreq.md common keys, service state, § 5 snapshot (`tasks` with § 2 `revision`; failed passes keep rows with `complete` false), `capacity` (limits, gate, admission, reservations), `requests`, `incident`, `agents` (§ 5, probed ≤ every 5 min), § 8 `access` and `reports` counts (rows: `reports.json`): `service status --json` ≤ 256 KB for 7 boards. `diagnostics`: selected `current` (CoreQ), health `healthy` (§8), lifecycle (consumer), CoreQ (Cargo.lock); [detail](service-version-producer.md#contract-detail). Capacity publishes on each sample; missing or older than `ttl` reads unknown ([specq](https://github.com/taskqorg/specq/blob/main/engineering/verification.md)). Failed publication is logged, blocks release-health marking and preserves supervision.
- Events: a pass appends to `<root>/.taskq/events` one line `<board time> #<N> close|failed|ask|blocked|gone <id>` per observed move (`blocked`: to `waiting`) and new `gone` entry; 30-day bound; offline moves are missing. An R6 stuck task (`review` too) logs `#N event stuck <state> <age>` once until it moves; [churn](profiling.md#contract-detail): >100 notes or >5 unaccepted spawns in 1 h. Detail: [menu](menu.md#contract-detail).

### Snapshot contract

Consumers (DashQ, the tray, PilotQ) read results, states and vocabularies from typed `status --json` fields and never re-parse board text (#2014, [research](../research/1981.md)).

- `verdict`: each task row carries the § 7 fields of the accepted `result` entry behind its current result: `changed` (text or null), `checks` (one string per `Checks:` line, in order) and `open` (text or null); continuation lines stay in their field, other fields and the card after a blank line are left out. A row without a current result carries null.
- `vocabulary`: `states`, one `{id, name, label}` per state in counter order (`label` is the canonical board label, `waiting` reads `q-blocked`), and `telemetry` with the request `errors`, `stage_errors` and `runtimes` codes. `status.json` carries the same object.
- Schema 1 grows only by new keys. `tests/fixtures/status-snapshot.schema.json` describes it; `tests/fixtures/status-snapshot.json` is the golden output (`status_json_matches_the_golden_fixture`, `TASKQ_BLESS=1` rewrites it) and must meet the schema (`status_json_meets_the_released_snapshot_schema`).
- Each release publishes both as `taskq-<version>-status-snapshot-schema.json` and `taskq-<version>-status-snapshot-fixtures.json`, covered by `SHA256SUMS` and provenance like the [answer card](answer-card.md) assets.
