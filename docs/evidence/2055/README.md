# Mac service capacity bursts (#2055)

R12 and § 7: find the producer before changing code. Sources: the Mac host log, `taskq profile export --since 4h` (taskq 4.6.54-dev) and the pinned coreq rev `1c23357cb6a59a08acf9e6c438f7ab15bc7f3c34`. Home paths are shown as `~`.

## A1. Producer of `process capacity`

`coreq::process::Permit::acquire` (coreq `src/process.rs`) holds one global pool of 16 permits per process (`CAPACITY = 16`, coreq R35). It never waits: the 17th concurrent `execute`/`exchange` returns `Failure::Capacity` at once, and its Display text is `process capacity`. coreq main has the same code.

The host service is one process. It runs one loop thread per registered project (about 13 on the Mac), plus host, link and tray threads, so all of them share those 16 permits. Each project pass starts board curls (`board.http`, 65 s/125 s deadlines), runtime probes (`runtime.version`, `runtime.login`) and git tidy (`cleanup.git*`). The link long poll keeps one more permit for most of its life. The reaper also keeps a permit until a killed child exits.

Telemetry over 4 h, capacity errors by operation: `board.http` 46, `cleanup.git` 8, `runtime.version` 4, `runtime.login` 4. Overlapping process operations in the export reach 20-28 at the worst seconds. At 07:29:50 about 34 `board.http` calls were live in the window.

The bursts follow dev-install restarts, when every project thread starts its first pass together:

| Service restart | First capacity error |
|---|---|
| 07:25:27 | 07:25:54 |
| 07:29:24 | 07:29:50 |
| 07:39:32 | 07:40:12 |
| 07:59:57 | 08:00:19 |

Smaller bursts also happen in steady state (07:39:02, and 00:24 and 05:45 earlier the same day).

Log lines from the two windows:

```
2026-10-31T07:25:54Z pass: curl: process capacity
2026-10-31T07:29:50Z runtime.version: `~/.local/bin/claude --version` failed: process capacity
2026-10-31T07:29:50Z (same for `claude auth status`, `codex --version`, `codex login status`)
2026-10-31T07:39:02Z pass: curl: process capacity
2026-10-31T07:40:12Z pass: curl: process capacity   (x3)
2026-10-31T08:00:19Z pass: curl: process capacity
```

Effect: a refused board curl fails the whole pass (`Service.pass` error) and the `taskq.run` call that issued it. A refused version or login probe makes the runtime look unavailable for that pass. Load was 2-3 workers, so worker count does not cause this. The number of threads that share one process pool causes it.

## A1. Holder behind `resources lock unavailable`

`src/resources.rs` guards `<install>/resource-state` with one flock and writes the holder pid next to it. Observers (`try_with`, `try_locked`, for example `lifecycle::pending` in the host loop) use a 0 ms wait. When the lock is busy, the host loop's lifecycle monitor reports an error. Host admission becomes `held`, and each project logs `capacity: ...; launches held` for one pass (R12 fail-safe).

```
2026-10-31T07:22:52Z capacity: resources lock unavailable: lock acquisition failed because the operation would block; waited 0 ms, held by pid 10607; launches held   (8 projects)
2026-10-31T08:00:18Z capacity: resources lock unavailable: ... waited 0 ms, held by pid 16686; launches held   (4 projects)
2026-10-31T08:00:19Z installation: installed host service (pid 1017) ... Install receipt f878be2c...
2026-10-31T07:43:40Z / 08:05:52Z ... held by pid 1017
```

- pid 16686: the `taskq service install` CLI that `scripts/dev-install.sh` runs. It writes the install receipt under the lock (see the 08:00:19 line).
- pid 10607: a short-lived taskq CLI during the dev install that started at 07:22:03; telemetry has a fenced `connection.http` at 07:22:44. The exact command is not confirmed: the sandbox blocks `ps`.
- pid 1017: the host service itself. Another of its threads held the lock while the host loop's 0 ms read failed. flock works per open file, so one process can contend with itself.

These holds last one pass and do not fail runs. They are noise beside the capacity errors. They stay as they are: reporting a busy lock as "unknown, hold launches" is R12; reusing a stale lifecycle value would weaken it.

## A2. Root cause and follow-up

Root cause: the coreq permit pool fails fast. The pool is shared process-wide, but each caller has its own deadline, and a refusal is a hard error to the caller. The fix belongs in coreq: `Permit::acquire` waits for a free permit within the call's own deadline and returns `Capacity` only after the deadline passes. The bound stays 16, as R35 requires ("at most 16 executions ... at once"), so no new ceiling or harness is added (A3). The coreq test changes from "full pool fails" to "full pool waits and succeeds after a release; a short deadline still fails".

Follow-up: [coreq#103](https://github.com/taskqorg/coreq/issues/103) (held). After its tag, taskq bumps the coreq and coreq-instrument rev in `Cargo.toml`. No taskq code change is needed: the callers already pass deadlines.
