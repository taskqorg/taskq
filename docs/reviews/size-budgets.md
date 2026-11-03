# Size budgets review (#2236)

Every size budget in taskq, with its usage against its cap, measured 2026-11-03 on branch `taskq-2236`.
The specq [size-budgets](https://github.com/taskqorg/specq/blob/main/engineering/contracts.md#size-budgets) rule applies: work under 70% of the cap; above the target, file one compaction task.
Rows marked **above 70%** need that task.

## Contract

| Budget | Cap | Usage | Share | Enforced by |
|---|---|---|---|---|
| `taskq.md` bytes | 79872 (declared `contract-budget`) | 55898 (main before: 61383) | 70.0% (target 55910) | specq contract lint via `check.py`; `core::tests::every_table_row_has_a_golden_trace` |
| `taskq.md` lines | 390 (declared) | 265 | 67.9% (target 273) | same |
| `taskq.md` line length | 400 visible characters | below cap | - | specq contract lint |
| Injected rules: header, contract, core, index (`RULES_CAP`, `src/runtime.rs`) | 131072 bytes | 121503 bytes in this attempt's input file (the file adds the task text) | 92.7%, **above 70%** | runtime refuses past the cap |

The injected-rules cap is the tightest budget. This change cuts about 5.5 KB of it. The rest comes from the specq core and index, so the compaction task belongs to specq or to the index selection in `src/runtime.rs`.

## CI and checks

| Budget | Cap | Usage | Share |
|---|---|---|---|
| Gate per runner (`check.py` `BUDGET`, [testing](../testing.md)) | 900 s | merge-group run 41551954549: windows-latest job 354 s, macos 291 s, windows-gnu 255 s, ubuntu 219 s | 39% at most |
| Fixture CLI exchange ([testing](../testing.md)) | 60 s Windows, 10 s elsewhere | per invocation | not aggregated |

## Messages (taskq.md R6, § 5, § 6, § 7)

These caps bind each message on its own, so no aggregate usage exists. The command or its test refuses an oversize message.

| Message | Cap |
|---|---|
| Ask card | ≤ 9 nonblank lines; line ≤ 160 characters; option ≤ 240; ≤ 2 links, ≤ 4 media |
| Option label / text | label ≤ 32 characters; text ≤ 160, one `★` |
| Report titles, reasons, options (R6) | ≤ 60, ≤ 160, ≤ 120 characters |
| Menu labels / lines | ≤ 32 / ≤ 160 characters |
| Errors | ≤ 2 lines × 160 characters |
| `blocker`/`failed` reason | ≤ 2 lines × 240 characters |
| `result` text | ≤ 12 lines × 240 characters |

## Retry counts per attempt

| Budget | Cap |
|---|---|
| Requeue (`REQUEUE_MAX`) | 3 |
| Gone (`GONE_MAX`) | 2 |
| Transient-failure steers (`CONTINUE_MAX`) | 2 |
| Background-job steers (`BACKGROUND_MAX`) | 3 |

## Stores and files

Project-local usage is from this Mac root. Install and host stores are outside a worker's read scope, so their usage is not measured here.

| Store | Cap | Usage |
|---|---|---|
| `.taskq/cli-errors.log` (`CLI_ERRORS_MAX`) | 256 KiB, newest half stays | 19785 bytes, 7.5% |
| `.taskq/inputs/*.txt` (`INPUTS`) | 50 newest plus locked files | 52, at the retention point (locked files stay) |
| Attempt logs (`ATTEMPT_LOGS`) | 50 newest plus live attempts | 52, at the retention point (live logs stay) |
| Incidents (`INCIDENTS`) | 50 | 0 |
| `taskq.log`, `<project>.log` (`LOG_MAX`) | 4 MiB, newest half stays | not measured |
| Telemetry line (`LINE_MAX`) / legacy store (`LEGACY_DB_MAX`) | 64 KiB / 28 MiB | not measured |
| Resource operation store (`MAX_OPS`) | 4096 operations, 24 h retention | not measured |
| Board op-id cache (`board::MAX`) | 4096 entries, 24 h | not measured |
| Focus file (`FOCUS_MAX`) | 4096 bytes | not measured |
| Child list (`KIDS_MAX`) | 1 MiB | not measured |
| DashQ enrollment tasks (`MAX_TASKS`) | 256 items | not measured |
| App control response / frame | 4 MiB / 1 MiB | per message |
| Native history read (`src/runtime.rs` `LIMIT`) | 8 MiB tail | per read |
| Doctor temp sweep (`MAX_TEMP`) | 2000 folders a pass | per pass |
| Host grant (`MAX_HOURS`) | 720 h | per grant |
