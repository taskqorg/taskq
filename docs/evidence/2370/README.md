# Mac live telemetry identity

Target: closed Mac task #2388, a real delivery verification. Worker attempt 6641795284 and reviewer attempt 6641841299 both ran on the Mac. No helper task was filed and nothing was rerun.

## A1. Trace

Command: `TASKQ_FIX=762d9db3 sh scripts/telemetry.sh trace 2388 --since 24h`, unchanged strict check, exit 0. [Trace output](trace-2388.txt) ends with `verdict: PASS (spawn to close traced for every attempt)`.

| Attempt | Role | Host | Runtime | PID | Lifecycle events |
| --- | --- | --- | --- | --- | --- |
| 6641795284 | work | mac | claude | 56377 | spawn, heartbeat, result, exit, slot-hold, slot-release |
| 6641841299 | review | mac | claude | 66476 | spawn, heartbeat, result, exit, slot-hold, slot-release |

## Source and runtime identity

The trace prints `contains 762d9db3: unknown` because a dev build has no release tag to compare. The source comes from these retained records instead of the installed marker alone:

- **Install log** (`d8295709….install.log`): `installed 4.6.79-261104-11 from d8295709; was 4.6.79-261104-10`, finished 20:15:51Z, with host service pid 23741.
- **Installed binary**: it embeds version `4.6.79-261104-11` and bundle id `1793822398-d82957095f9e`. The bundle's epoch 1793822398 is 19:59:58Z, the commit time of d8295709.
- **Service log**: `current is 4.6.79-261104-11; restarting` at 20:14:29Z, then `taskq 4.6.79-261104-11 service on mac for taskqorg/taskq` at 20:14:44Z. No later restart appears through the close at 20:56:26Z.
- **Telemetry rows**: all 17 worker and stage rows of attempt 6641795284 and all 17 of attempt 6641841299 carry version `4.6.79-261104-11`. The first spawn is at 20:51:18Z, after the install.
- **Service process**: `health` at 21:24Z reports service pid 23741, the pid the install log recorded, so that process served both attempts.
- **Git ancestry**: `git merge-base --is-ancestor` exits 0 for 762d9db3 and for 1e5ff561 against d8295709.

## A2. Health

Command: `TASKQ_FIX=762d9db3 sh scripts/telemetry.sh health` (default 150-second window), run on the same build. [Health output](health.txt) ends with verdict PASS: no stall errors, no `legacy_offset` errors, no re-baselines. The earlier Mac run on 4.6.77-261104-6 is in [docs/evidence/2374](../2374/README.md). A finite window proves observed import health, not indefinite future health.

## Reading the output

Local paths are removed from the release line of both files; every other line is verbatim. Both attempts also record `alive-after-result` and `kill` rows with outcome `failed` and reason `pid-alive-after-result`. Each process outlived its hand-in, was stopped, and then recorded `exit-0`. The strict check does not treat these rows as a failure and A1 does not require them to be absent.
