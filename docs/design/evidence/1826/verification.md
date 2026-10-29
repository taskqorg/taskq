# Reports acceptance (#1826)

Finished research delivers a reviewed report. The owner can read it later and create held follow-ups without keeping finished research in the queue.

## Contract and scope

- The owner answer on #1826 authorizes the research gate changes in §7 and §7 Result and close verdicts. R13's linked rule-ID detail accepts ordinary section citations while preserving section/subsection boundaries.
- R1 is preserved: trusted issue comments own read/decision state and immutable create claims; there is no report database or local decision store. Reports do not change §2 queue transitions, admission, focus or capacity.
- The implementation uses the same Reports view in Menu and Conversations, exact result-SHA report content, close-time pagination and SHA-bound decisions. Make tasks validates proposals and creation prerequisites, then creates linked held drafts through the existing §1 claims. Unknown writes retain the report and any created issue numbers.
- Final implementation is rebased on `9683b9ae586ca17a7d2d51977cf47b621b522402`, preserving the shared Conversations changes. The native capture manifest pins its source commit and binary hash. Earlier checks below name the previous base where applicable; they do not qualify the rebased candidate.

## Verification

| Check | Observation | Scope |
|---|---|---|
| `cargo test --locked --bin taskq reports` | 20 passed, 1 ignored | Reports model, native board adapters, worker briefs, IPC and snapshot selection; ignored process fixture is not a pass |
| `node tests/reports.test.cjs` | Passed | Source-bound receipts, read, Later, Drop, held creation dispatch, stale/unknown/partial outcomes and stable cursor |
| `node tests/reports.browser.cjs` | Passed 16 cells and both native-fixture bridge cases | Real bundles with invented IPC; Menu/Conversations, dark/light, 350/940 px, 16/20 px; separate from native acceptance |
| All `tests/*.test.cjs` | Passed | Existing component, effect-inventory, source-hash and IPC guards retained |
| `python3 scripts/check.test.py` and `python3 scripts/check.py --rules --base bbeeae9af4acff3a4148def94e054ce5cf80f32c` | Passed | Natural section citation accepted; wrong section and unnamed rule changes still refused |
| `sh docs/shots/check.test.sh` | Passed | Missing/malformed manifests and unrecorded images still fail; stale sources remain detectable, including the new Reports module |
| `python3 scripts/check.py --base bbeeae9af4acff3a4148def94e054ce5cf80f32c` | Failed: 590 passed, 19 failed, 37 ignored; 316.468 s | Windows worker, warm debug build; 18 owner-operation failures are worker-context blocked, one native IPC timeout is unexplained. Later integration/offline/Node stages did not run in this gate |

The blocked checks exercise host installation, host policy, lifecycle, resources, tray capacity and Windows security restoration. The worker refused their owner operations. No permissions, worker identity, budgets or assertions were weakened. The separate `runtime::app_control::windows_pipe::tests::native_pipe_bounds_unread_writes_and_empty_reads` failure was a native peer timeout. The same compiled test passed alone in 0.20 s; its failed full-run observation remains unexplained. R12 keeps the overall gate failed and publication dependent on green merge-result CI.

## Before and after

- Before implementation, the new Reports component check failed because its shared module was absent. The component and browser checks subsequently passed with the real bundles and explicit invented board receipts.
- The citation regression first failed because `section 7` did not name `§7`; after the parser fix it passed without allowing `section 70` or a subsection to approve its parent.
- The creation-prerequisite regression failed before the fix: a rejected Make wrote a fourth report comment instead of preserving the original three. After moving shared validation before the make-intent, the same assertion passed for missing configuration and a queue issue used as the claim target; Drop remained available. Immutable claims and pending-write fencing remain protected.
- A duplicate-board registration regression failed with two reads instead of one. The shared view now reads each board once, keeps Drop effective across aliases, hides cached rows from removed registrations and reports uncollected projects as unknown rather than empty. The same component check passed after the fix.
- The first native Reports route produced the ordinary Menu with Reports collapsed. The fixture had been evaluated without its pushed-state argument. Its invocation was corrected; final native recapture remains pending.
- Read-only review confirmed the reservation/race, discovery, pagination, receipt-preservation and pending-open fixes, then accepted the bounded prerequisite correction. This review did not substitute for tests or native evidence.

## Remaining acceptance

Windows native recapture and screenshot provenance refresh are pending. macOS WKWebView light/dark evidence is unavailable on this Windows host. A6 remains incomplete until both native renderers are checked on the same source. No installed-host acceptance, deployment, merge or green merge-group CI is claimed.
