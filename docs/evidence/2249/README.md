# Live telemetry proof on mac, win and pds18

A1 and A2 hold for one traced task and one observed health window per host (R5, R12). Each host's own worker produced its proof, as the owner chose on this task. This file adds the installed Windows proof and links the other two hosts. No implementation, contract, host policy or runtime setting changed.

| Host | A1 trace | A2 health | Evidence |
| --- | --- | --- | --- |
| win | #2354, PASS on installed 4.6.78 | PASS on installed 4.6.78 | this directory |
| mac | #2388, PASS on 4.6.79-261104-11 | PASS on 4.6.79-261104-11 | [docs/evidence/2370](../2370/README.md) |
| pds18 | #2347, PASS on 4.6.73 | PASS on 4.6.73 | [docs/evidence/2284](../2284/README.md) |

## A1 on win

Command: `TASKQ_FIX=762d9db3 sh scripts/telemetry.sh trace 2354 --since 24h`, exit 0. [Trace output](trace-2354.txt) ends with `verdict: PASS (spawn to close traced for every attempt)`.

| Attempt | Role | Host | Runtime | PID | Lifecycle events |
| --- | --- | --- | --- | --- | --- |
| 6635146807 | work | win | codex | 27092 | spawn, heartbeat, result, exit, slot-hold, slot-release |
| 6635704317 | review | win | codex | 42800 | spawn, heartbeat, result, exit, slot-hold, slot-release |

The reader is installed 4.6.78, which contains #2237, the release PID fix `762d9db3` and the export fix `026db252`. Installed 4.6.76 failed the same command with `invalid uri authority: %3f`, as recorded earlier on this task. The service that emitted these rows ran 4.6.76, which contains `762d9db3`. All 67 exported rows of both attempts carry version 4.6.76.

## A2 on win

Command: `TASKQ_FIX=762d9db3 sh scripts/telemetry.sh health`, default 150-second window, exit 0. [Health output](health.txt) ends with verdict PASS: no stall errors, no `legacy_offset` errors and no re-baselines. A finite window proves observed import health, not indefinite future health.

`watch` reads Board unknown because of 1 recorded recovery gap. The script discloses this gap and does not count it as a stall.

## Open gap: Claude executor PID on win

[Trace of #2391](trace-2391.txt) fails on installed 4.6.78, exit 1. Reviewer attempt 6641725045 ran on Claude and records PID 0 on its slot-hold, spawn and first heartbeat rows. The script reports `invalid identity fields process_id`. This trace is the only one here from a service already running 4.6.78.

The fix is #2392 (`d8295709`), which merged after the 4.6.78 tag and is on main but not on `live`. The Mac proof shows it working on a dev build from that commit. No Windows run of it was observed, so the Claude executor PID on win remains unknown (R12). The PASS above covers a Codex worker and reviewer only.

## Reading the output

Local paths are removed from the release line of each file; every other line is verbatim. Reviewer attempt 6635704317 also records `alive-after-result` and `kill` rows with outcome `failed`, then `exit--1`. The strict check does not treat these rows as a failure and A1 does not require them to be absent. Close transitions correlate by project and task, so they have no attempt.
