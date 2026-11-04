# Live telemetry proof on pds18

Both acceptance items are confirmed on pds18.ufobe.com with installed TaskQ 4.6.69, whose release tag contains #2280 (`1e5ff561`). This is native installed-host observation, not a simulated lifecycle. No implementation, contract, host policy or runtime settings changed.

## A1: spawn to close

Command: `sh scripts/telemetry.sh trace 2318 --since 2h --project taskqorg/taskq`.

[Trace output](trace-2318.txt) passes. [Retained export](export-2318.txt) contains the export header and only task 2318's worker, stage and transition records for `taskqorg/taskq`; other tasks are omitted. The trace uses `profile export` alone, without board state or process inspection.

On 2026-11-04 UTC:

| Attempt | Role | Runtime | Executor pid | Spawn | Result (service) | Exit | Slot release |
|---|---|---|---|---|---|---|---|
| 6623220950 | worker | codex | 251613 | 05:17:13 | 05:18:16 | 05:18:19 | 05:18:19 |
| 6623257146 | reviewer | codex | 262071 | 05:19:20 | 05:20:03 | 05:20:06 | 05:20:06 |

Both attempts have slot-hold at spawn and heartbeat while running. Every worker-kind lifecycle record carries task, attempt, host, runtime, process_id and reason (`to`). Reasons include `claimed`, `running`, `handed-in`, `pid-alive-after-result`, `exit-0` and `execution-released`. The final transition is `merge-queue->closed` at 05:20:24.

Identity limits remain explicit (R12): CLI result receipts record process_id `2`; service result and exit records retain the executor pid. Slot-release records use process_id `0` after execution release, so they correlate by task/attempt/host/runtime rather than a retained executor pid. The close transition has no attempt, host or runtime; it correlates by project/task. Unknown stage/transition outcomes remain unknown. The worker's alive-after-result failure and subsequent kill/exit are retained, not hidden by the final PASS. This proves the recorded lifecycle and its identity fields, not that each record contains a live executor pid or that every lifecycle was uneventful.

[Earlier checkpoint](checkpoint-trace-2318.txt) failed because review had not finished; [its export](checkpoint-export-2318.txt) retains that partial observation. The later trace passes after natural review completion. This is completion of an observation window, not a code fix or regression claim. R12 prevented claiming completion from the earlier worker-only trace; §7.3 provided the timed continuation.

## A2: legacy import health

Command: `sh scripts/telemetry.sh health` (default 150-second window). Fresh output is retained in [health output](health.txt). Earlier A2 evidence remains in the task's accepted stage entries. The finite window proves the observed import health, not indefinite future health.

The health command separately reports service telemetry unknown (`service pid 23349 not running`) and one failed board call out of 11,685. Neither is treated as healthy or attributed to an import stall. No host repair was attempted. This proof concerns A2's legacy import checks; it does not establish overall service or board health.

## Verification scope

R5: this evidence accompanies the task result. R12: live records and limitations are preserved separately from repository checks. R13: no decision changes were needed. Repository validation uses `python3 scripts/check.py` after rebasing, with documentation-only selection under `docs/testing.md`; it does not substitute for the installed-host trace.
