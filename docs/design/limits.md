# Hardcoded limits

Every hardcoded deadline, retry, poll cadence, cache age and size bound in `src/`, with the failure it prevents (#2239).

## Contract detail

Contract detail for taskq.md sections 3, 4, 5 and 8, linked from [host service](host-service.md#contract-detail).

Rule: a wait has no deadline of its own unless it prevents a concrete failure named below. A child that bounds its own waits is waited for until it exits: this binary's own CLI (its board calls, locks and pacing are bounded inside it) and an owner-run project script. `platform::complete` is that wait. A new limit joins one class below in the same change, with its reason; a limit that fits no class is removed. Test-only values (`#[cfg(test)]`) and OS constants (priority classes, access masks, `GIB`) are not limits.

### Removed

These had no failure to prevent; each now waits for its child to exit.

- `tray.answer-command` (30 s status read, 60 s write), `tray.history-read` (60 s), `tray.reports-command` (180 s), `tray.recovery` (60 s), `tray.selftest` (30 s): the own CLI already bounds its board and lock waits; the tray deadline only killed a write mid-flight and left its outcome unknown. Each action kind has its own tray queue, so a long command delays only later commands of the same kind.
- `tray.install` (60 s): `service install` waits up to its own handover deadlines (300 s and 60 s below), so the tray killed valid installs.
- `host.setup` (300 s): the project's `host_setup` script runs once per host from the owner's `service install`; it takes as long as the project needs and the owner can interrupt it.

### Contract-named

Named with their reason in taskq.md or another contract detail; this page does not restate them.

- § 2: `core` `REQUEUE_MAX` 3 and `GONE_MAX` 2; `summary` `FILES` 3.
- § 4: `main` `TTL` 300 s (heartbeat) and `MENU_POLL` 15 s; `service` `CONTINUE_MAX` 2 and `BACKGROUND_MAX` 3; `board/github` `OPEN_REUSE` 300 s (board list lag).
- R6: `snapshot` `STUCK_AFTER` 2 h; [profiling](profiling.md#contract-detail): `snapshot` `CHURN_NOTES` 100 and `CHURN_SPAWNS` 5.
- M14 ([menu](menu.md#contract-detail)): `notify` `BURST` 3 per `WINDOW` 10 s.
- [Host service data](host-service.md#contract-detail): `data` `LOG_MAX` 4 MiB, `CLI_ERRORS_MAX` 256 KiB, `ATTEMPT_LOGS`, `INCIDENTS` and `INPUTS` 50 each; `telemetry` `WINDOW_MS` 30 days; tray collector retry 30 s.
- [Host doctor](host-service.md#host-doctor): `doctor` `FIRST` 10 min, `EVERY` 1 h, `KEEP_PREVIOUS` 2, `STALE` 1 day, `MAX_TEMP` 2000.
- [Capacity](capacity.md#contract-detail): `admission` `SPACING` 30 s (one Auto launch per 30 s host-wide); capacity samples older than 60 s read unknown.
- [Enrollment](enrollment.md): `enrollment` `MAX_TASKS` 256 (DashQ C4), `RESEND` 300 s, `LOGIN_VALID` 600 s. [Host link](host-link.md): `link` `TTL` 60 s and `BODY` 256 KiB (dashq#234 relay limits); `PAGE` 180 KiB is the page that still fits `BODY` sealed.

### Admission

A new worker on a host short of memory pushes it into swap and slows every running worker.

- `service` `FLOOR_GIB` 2.5 GiB available and macOS `PRESSURE_CRITICAL` level 4 hold Auto admission (D03, [admission calibration](admission-calibration.md)).
- `admission` `STALE` 120 s: a reservation entry not rewritten for that long belongs to a stopped service and is released, so a crashed service cannot hold slots forever.

### Service pass budget

A stalled step stalls the whole host: heartbeats age past `TTL`, § 3 leases go stale and other hosts reclaim live work.

- `service` `NATIVE_PASS_BUDGET` 20 s: native runtime observation and reaping inside one pass; the rest carries to the next pass.
- `service` `seed.git` 60 s and `seed.cp` 300 s: a slower volume skips seeding instead of holding the pass.
- `tidy` git 120 s; `reports` `reports.candidate` git 15 s; `recovery` `recovery.workspace` git 10 s; `runtime` worktree git 30 s.
- `doctor` `NATIVE_BUDGET` 60 s: Scheduled Tasks, `systemctl` and `launchctl` listings on the doctor thread; a hung service manager must not keep the single doctor pass running forever.
- `health` `ASK_WAIT` 300 s with `ASK_POLL` 20 s: the cross-host health question waits for an answer, then reports the host unknown.

### External tool and network deadlines

A tool can hang (network stall, OS prompt, locked keychain). Without a deadline the caller (service pass, tray queue, owner command) never returns an outcome. CoreQ's process API also refuses a budget above 1 h.

- Board HTTP: `board.http` 65 s for a request and 125 s for an upload (curl `max-time` 60 s and 120 s, plus 5 s for curl to report); `link` curl `WAIT` 5 s plus the long-poll wait plus 5 s; `rules.download` 15 s with curl `--max-time 10`.
- Version and identity probes: `tool.version` 10 s, `host.identity` 10 s, runtime `probe_within`/`probe_env` 10 s, `runtime.app-presence` 10 s, `runtime.open-link` 10 s, `runtime.taskkill` 10 s, `tray.theme` 10 s.
- Signing and macOS tools: `signing.identity`, `signing.command`, `signing.adhoc`, `lsregister`, `codesign --verify` and the TCC log query 30 s each; `tray.agentq-screencapture` 5 s.
- Release and dev: `release.selftest` 30 s (a candidate that hangs is not installed), `dev.main` `git ls-remote` 30 s.
- Native service manager: `host_control` commands 30 s (120 s in tests: cold PowerShell on loaded CI runners, #1673), `host.uninstall.readback` 30 s, `host.stop.dispatch` 120 s (`RuntimeMaxSec=120` on Linux).
- `windows_sandbox` preflight 30 s; `tray.resources.submit` 30 s (submits a terminal; the terminal's own lifetime is not bounded).
- Runtime launch: `setup.codex-launch` 60 s, `background_launch` 30 s, `app_queue` 10 s; app IPC pipe read and write 10 s each, app cancel 10 s.

### Observation deadlines

Wait for another process to do something it does not announce. Past the deadline the caller says the outcome is unknown and what to do, instead of hanging when the other side is gone or stopped.

- `host_install`: upgrade handover 300 s, native ownership 60 s, project registration 60 s.
- `main` capacity and channel apply: 20 reads 1 s apart, then "Saved; waiting for host service" (the host may be stopped; the saved file applies later).
- `host_control` `wait_exit` 10 s; `windows_process` kill confirmation 2 s; `runtime` observation deadlines 5 s and 10 s; `owner` launch readback 5 s; `owner_stream` receipt 10 s; `app_control` coordination 100 ms and 250 ms.
- `board/gitlab` `MERGE_STATUS_READS` 5 reads `MERGE_STATUS_WAIT` 2 s apart (GitLab computes merge status asynchronously); `PIPELINE_DEADLINE` 24 h for a pipeline that never reports.
- `pending` `CONFIRMED` 60 s, `MISMATCH` 30 s, `KEEP` 10 min: how long a tray write waits for the board readback before it shows unconfirmed.
- Tray pending command expiry 30 s (paused across a stalled event loop over 5 s).
- `platform::drain_telemetry` 20 s and `telemetry::settle` 10 s / 30 s: telemetry flush at exit (#1566); the process must still exit.

### Lock waits

A lock holder may be another live process. A bounded wait returns "busy, retry" instead of blocking an owner command behind a long pass.

- `host` `LOCAL_LOCK_WAIT` 10 s (#1351); `resources` `WAIT` 10 s (also the update-lock wait for install, #1415); `protected_scope` `WAIT` 5 s; `profile` `REPAIR_READ` 300 s (#1341) and log lock 1 s; `telemetry` SQLite `busy_timeout` 1 s and writer heartbeat 10 s.

### Poll cadences

No change notification exists for the watched state, so it is read again at a fixed spacing.

- Host loop: 1 s sleeps between local ownership, stop and poke checks; the configured `poll` clamped to 5–60 s; link reads 10 s apart; `link` backoff 5–300 s.
- `health` `EVERY` 1 h and `STALE` 3× that (#2352); `tidy` `EVERY` 1 h and `UNOWNED_AFTER` 7 days; `incoming` and `reports` `FULL_SCAN` 1800 s (#2352) and `reports` `LIST_LAG` 300 s (board list lag); `incoming` `READ_EVERY` 180 s and `reports` collection every 3×`poll`, under `ttl` (#2352); `incoming` `EVENT_READS` 5.
- `resources` `SPACING` 5 s, `IDLE` 30 s, `FRESH` 60 s, `AHEAD` 30 s, `RECONCILE` 300 s; `telemetry_sync` `RUN` 30 s, `IDLE` 60 s, `STOP` 300 s.
- `agentq` runtime list refresh 300 s (a probe takes about 1 s, #1591) and readback 2 s; tray tick 2 s; `owner_stream` 25–100 ms and 1 s; `diagnostic` control check 1 s; lock retry sleeps 10–100 ms.

### Caches and probe ages

Avoid repeating a slow native probe or board read on every pass or render.

- `runtime/headroom` `TTL` 300 s; `runtime/accounts` `PROBE` 1800 s; `service` `USAGE_PROBE` 1800 s and `SETTLE` 30 s (#1718); runtime history version cache 300 s.
- `board` ETag cache `ETAGS` 256 entries, `AGE` 1 day, `MAX` 4096; `board` `CLOSED_PAGE` 10.
- `conversations` catalog refresh 2 s and loading cutoff 4 s.

### Retries

A transient fault (busy file, sharing violation, network blip) clears by itself; a bounded retry turns it into success without looping on a permanent fault.

- `enrollment` retry 2 s apart; test-dir removal 50 tries 100 ms apart on Windows sharing violations; `requests` `MAX_PAUSE` 1 h and `MAX_PACE` 5 (board rate-limit pacing, bounded by the board's reset time).

### Size bounds

Bound memory, disk growth or an external protocol limit.

- Protocol: `board/reply` `PART` 60000 bytes (GitHub comment limit); `app_control` `RESPONSE_LIMIT` 4 MiB and `FRAME_LIMIT` 1 MiB; `windows_pipe` writes 4096 bytes per call.
- Inputs read from disk: `data` `FOCUS_MAX` 4096 and `KIDS_MAX` 1 MiB; `runtime` `RULES_CAP` 128 KiB and transcript `LIMIT` 8 MiB; `pm` `BODY` 1 MiB; `resources` `BODY` 16 KiB and `ITEMS` 8; small state files read at most 1 KiB to 64 KiB.
- Disk growth: `conversations` `FILE_BYTES` 5 MiB per attachment and `STORE_BYTES` 100 MiB; `telemetry` `LINE_MAX` 64 KiB and `LEGACY_DB_MAX` 28 MiB; `telemetry_sync` `BLOCK` 512 KiB, `RAW` 1 MiB, `BLOCKS` 8; `external_slots` and `resources` `RETAIN` 24 h; `resources` `MAX_OPS` 4096 and `MAX_HOURS` 720; `incoming` and `service` `MONTH`.
- Display: `snapshot` `OPS` 16 and `ANSWER_CHARS` 240; `requests` `TOP` 8; one-line reasons cut at 160 characters, titles at 40–80; history lists of 20.
- Windows: `setup` `LINK_WINDOW_MS` 10 min; `diagnostic` `HEALTH_WINDOW_MS` 15 min; tray blur and focus timings 300–800 ms (window-manager focus races).
