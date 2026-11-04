# Live telemetry proof on pds18

A1 and A2 are confirmed for the observed windows (R12). The captured lifecycle was emitted on pds18.ufobe.com by TaskQ 4.6.71, and fresh readback runs 4.6.73; both release tags contain #2280 (`1e5ff561`) and the release-PID correction #2337 (`762d9db3`). No implementation, contract, host policy or runtime settings changed in this proof.

## A1: spawn to close

Command: `sh scripts/telemetry.sh trace 2347 --since 6h --project taskqorg/taskq` using the corrected script from main. [Trace output](trace-2347.txt) passes. [Filtered export](export-2347.txt) retains the export header and only task 2347's worker, stage and transition observations for `taskqorg/taskq`. Both commands use installed `profile export`, without board state or process inspection. The live capture was taken on 2026-11-04 around 12:02 UTC; the separate filtered export was captured immediately before it.

| Attempt | Role | Runtime | Executor PID | Spawn UTC | Service result UTC | Exit and slot-release UTC |
|---|---|---|---|---|---|---|
| 6626599210 | worker | codex | 1351368 | 08:46:07 | 08:51:20, 08:52:22 | 08:52:23 |
| 6626715183 | reviewer | codex | 1393673 | 08:52:31 | 08:53:30 | 08:53:32 |

For each attempt, spawn, heartbeat, result, exit, slot-hold and slot-release retain task, attempt, host, runtime, executor PID and reason. Slot-hold is at spawn; heartbeat occurs while running. A separate readback of the retained export required all six events with the same spawn PID and nonempty identity/reason fields for each attempt; both passed. CLI result receipts use PID 2; service result records separately retain the executor PID. Reasons include `claimed`, `running`, `handed-in`, `exit--1`, `exit-0` and `execution-released`.

The trace records review to merge-queue, merge-queue to ask, ask to later at 10:57:38, and later to closed at 10:57:49 UTC. This is an observed task closure, not evidence that #2347 delivered its change. The close transition correlates by project/task; it has no attempt, runtime or host. The six worker lifecycle events supply those identities. The worker's alive-after-result failure, kill request and failed exit are retained; the reviewer exits successfully. Unknown stage/transition outcomes stay unknown. Trace PASS establishes retained lifecycle coverage, not successful execution or overall service health.

## Before and after the PID fix

[Before-fix export](export-2318.txt) for #2318 on 4.6.69 has slot-release PID 0 instead of executor PIDs 251613 and 262071. Its [old trace](trace-2318.txt) reported PASS because the old script checked event presence only; that PASS did not satisfy A1. #2337 corrected PID retention and trace identity validation. The new #2347 export above retains PIDs 1351368 and 1393673 through release and passes both the corrected script and same-executor readback (R12, §7.4).

The [post-fix checkpoint trace](checkpoint-trace-2347.txt) and [checkpoint export](checkpoint-export-2347.txt) on 4.6.71 already had correct PIDs, but lacked closure and correctly remained unknown. The fresh trace adds natural closure. The earlier #2318 [checkpoint](checkpoint-trace-2318.txt) and [export](checkpoint-export-2318.txt) also preserve the original incomplete observation. Timed continuations followed §7.3; none treated a running reviewer as completion.

## A2: legacy import health

Command: `sh scripts/telemetry.sh health` (default 150-second window). [Fresh health output](health-current.txt) retains the observation on 4.6.73; [earlier health output](health.txt) and accepted stage entries preserve the previous A2 proof. The fresh window reports zero stall errors, legacy_offset errors and re-baselines. Export succeeds with 85,933 rows, legacy source absent, zero legacy samples and zero recorded recovery gaps. The finite window proves observed import health, not indefinite future health.

Service telemetry is separately unknown (`service pid 23349 not running`). Board watch reports 322 failed calls out of 12,337 (2.61%); CLI collection reports zero lost observations and failed batches, with timeouts uncounted. None of these observations is attributed to an import stall. This proof concerns the legacy import checks and does not establish overall service or board health. No host repair was attempted.

## Verification scope

R5: this evidence accompanies the task result. R12: installed-host observations, before-fix failure and after-fix pass remain separate from repository validation. R13: no decision changes were needed. `python3 scripts/check.py` validates this documentation candidate under `docs/testing.md`; it does not substitute for the live trace. Publication requires green merge-result CI (§7); local checks do not establish that gate.
