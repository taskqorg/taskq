# Capacity: limits, controls and load gating

Current host-policy authority is [taskq.md sections3/9](../../taskq.md#3-attempts-lease-capacity) and [host-service acceptance](host-service.md#command-surface-acceptance).

## Host spillover

Contract detail for R9/R12/R13 and taskq.md sections 4/9. Host-owned `claim` is `any` (a primary host, the default), `spillover`, `labelled` (pinned-only) or `none` (manager-only). Without weights, primary claims unlabelled tasks immediately; pinned-only claims only `host-<me>`; manager-only claims neither workers nor reviewers, including host-pinned tasks. Manager/owner conversations and external manager slots remain available under their existing gates. Explicit `host-*` always excludes other hosts; spillover claims its own label immediately. Existing attempts keep their runtime, account and ownership.

Spillover takes unlabelled ready work and new reviews when all observed primary hosts have zero effective availability for the task. It inspects primaries first: a fresh primary with availability, or one only waiting out temporary launch spacing (`launch-interval`), holds spillover even after grace. `spillover_grace` seconds (host.json, nonnegative integer, default 300) release only a hold caused by an unknown primary observation. Measure grace from the last transition into the current ready/review state, otherwise issue creation, using board time. Fresh primary availability includes host/project remaining slots, runtime/account enablement, quota windows, login, grants, model/effort restrictions, capabilities, focus and common admission holds. A spare eligible subscription counts as available. A stale, absent, incompatible or corrupt primary observation is unknown, never zero; before grace it holds spillover. No primary observations also means unknown. Grace permits a new claim only; execution fencing still forbids duplicate or migrated work.

Each board heartbeat carries `balancing <JSON>` schema 1 (unweighted) or 2 (weighted): canonical `mode`, project `default_runtime`, optional `allowed_runtimes`, `auto_reroute`, `has`, nullable `focus`, project `routing`, and per-runtime `code`, bounded `reason`, complete `quota` window rows and effective `full` grant. Unknown focus is null. Schema 2 adds `weights`, the latest accepted local unlabelled worker `last_spawn` comment ID (retained as a round-robin hint after task closure), and per-runtime `load` with occupied `running` slots (including retained/unknown executions and reservations), effective `free` slots and nullable quota `headroom` fraction; `load` is present whenever host occupancy is known, also under a refusal (`free` 0) or unknown headroom (`headroom` null), and only `admitted` runtimes rank. Schema 1 readers treat schema 2 as incompatible. `code: admitted` means at least one effective free slot; effective free slots include host/project/runtime and aggregate ceilings. Every other known refusal means zero effective availability, except launch spacing in `load-gate`, which is waiting; `unknown` stays unknown. Consumers apply task routing, capability and focus constraints to this projection, age it from board `updated_at` and require freshness within `ttl`. It contains no credentials, paths or raw configuration. Old readers ignore the line; all board hosts must upgrade before spillover is enabled. Admission/status records retain the primary wait or quota reset/probe reason and actual fallback runtime. Saved limits never encode temporary quota holds.

Host-owned `projects.<root>` may contain optional `limits`, `default_runtime` (CLI only), `allowed_runtimes` (distinct known runtime IDs, empty disables task execution), `auto_reroute` (boolean, default false), and the existing `op`; an entry needs at least one policy field. Missing execution fields inherit host defaults/all runtimes. These policies apply to new workers and reviewers, never migrate active attempts. Allowed runtimes only narrow eligibility; limits remain lower-only. Project defaults affect tasks, not owner/manager sessions. Invalid policy holds launches as unknown.

The owner-enabled `auto_reroute` exception removes a CLI `run-*` constraint only when that runtime is explicitly disabled by a host/project zero limit, excluded from `allowed_runtimes`, or has every account operator-disabled. It then uses the normal project preference/default and eligible CLI fallback; it never rewrites the board label or changes limits, accounts, grants, models or effort. Missing login, quota, full slots and unknown availability do not authorize removal of a pin. `run-codex-app` remains opt-in and strict. Without the exception, every pin remains strict. Runtime selection and readiness share this policy; spawns and admission reasons identify the ignored pin and actual runtime.

## Owner weights and quota headroom

Contract detail for [§4 Balancing](../../taskq.md#balancing) (R8, R13).

Owner runtime `weights` in host.json are integers 0..1000000 for CLI runtimes. Weight 0 never picks that runtime for unpinned work or review, fallback included, while `run-*` pins and owner sessions naming it still run there.
`taskq balance claude=2 codex=1` replaces this local host's weights, `taskq balance --clear` restores preference/default routing. Workers cannot write them. Weights never change limits, grants, accounts or active attempts (R9).
Unpinned new workers/reviewers choose the eligible weighted runtime with lowest running / effective weight. Effective weight is owner weight times the smallest fresh quota remaining fraction (floor 1%); ties prefer greater effective weight, then soft routing. Missing/stale headroom is unknown and uses unchanged weight; holds still exclude. Unweighted runtimes keep existing fallback.
Unlabelled ready work chooses fresh eligible primary hosts by greatest sum of free slots times effective runtime weights. Ties rotate after the last trusted primary spawn host, initially host-name order. No weights keeps existing host claims. Stale/incompatible hosts are unknown and excluded from weighted spread, never reported as zero.
Host pins, strict runtime pins and the existing auto_reroute exception, spillover, focus, grants, common gates and §3 fencing remain binding. Status shows per-host occupancy, weights and headroom; admission records actual runtime and pick reason. Compatible readers are required before enabling weights (§9). [Detail](#owner-weights-and-quota-headroom); four sections, `work` aliases: [configuration](configuration.md#balancing).

Weights replace soft preference ranking when configured; stable runtime ties retain that preference. Unlisted CLI runtimes remain fallback after weighted candidates. Each candidate must still pass the existing admission checks; a held runtime cannot win by weight. Unknown common admission state still refuses. The explicit owner-enabled disabled-pin exception is unchanged.

The service reads quota windows of its own attempt logs through AgentQ `log_limits`, which returns every window with used fraction, reset and native event ID (background jobs without limit records read none). It retains the most recent observation per runtime/account/window in `<install>/quota-headroom.json`, using the host account lock. An attempt-log `.headroom` marker records its processed event IDs, so concurrent logs for one account cannot refresh each other's old observations. The existing attempt-log pruner removes orphaned markers; retained/live logs keep theirs. Re-reading the same event never refreshes its time. Only the account selected for a new launch contributes. Fractions expire after 300 seconds or the reported reset, whichever comes first; future, missing or stale observations are unknown. Unknown uses neutral weight; the heartbeat names its cause on a separate `headroom <rt> <cause>` line, which N-1 readers ignore, never as a new `load` field. Corrupt/unreadable observation storage refuses weighted launches. These observations never clear a quota hold, disable an account or change a capacity limit.

Weighted primary spread uses only trusted, fresh, task-eligible heartbeat projections plus the current local projection. It sums free slots times effective weights only for eligible runtimes, uses the latest accepted primary spawn in board comment order as the tie cursor, and recomputes before each claim. Host-pinned tasks bypass spread; spillover retains its existing zero-availability/grace policy. Hosts with stale/incompatible/unknown projections are excluded with an unknown diagnostic, never assigned zero capacity. The board's existing read-after-write claim fence remains the final authority under stale projections or concurrent decisions.

## Status

The local `status.json` holds one `capacity` object:

| Field | Meaning |
|---|---|
| `limits`, `used` | Effective runtime ceilings and actual live-child counts, including zero and over-limit values |
| `load_gate` | Effective `enabled`, `cpu_max`, `memory_min` settings |
| `gate` | `off`, `open`, `held`, or `unknown`; independent of focus and full slots |
| `reasons` | All blockers: `disk`, `cpu`, `memory`, `sample`, `config`, or `cooldown`, with a short readable explanation |
| `cpu_percent`, `memory_available_percent` | Latest raw percentages, null when unavailable |
| `disk_free_bytes` | Free bytes of the [disk guard](#disk-guard) reading, null when unread |
| `sample_at` | Wall-clock timestamp for display; null without a valid sample |
| `sample_age_seconds` | Age measured with the service's monotonic clock at status publication |

Use existing `host`, `project`, `at` and `pid` to identify/freshen the snapshot. Missing new fields mean `Unknown: service has no capacity status` ([unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy)).
Runtime rows come from the union of effective limits and live children; a removed runtime with a child still shows usage and limit 0.
Update the capacity snapshot every sample and after launch/reap/reload, without forcing a board poll every sample. Mark a sample/status older than 60 seconds unknown; add elapsed display time to published age, and treat clock anomalies as unknown.
The tray reads only registered local project snapshots, no board/token. Remote hosts show `Unknown: no local snapshot`; do not invent fleet usage from heartbeat task counts. A future remote transport is separate work.

## Load model

The gate reads CPU busy percentage across all logical processors and physical memory available percentage. Both reflect all host activity, including other projects and unrelated programs.
CPU load average is not CPU utilization and has incompatible Windows semantics; per-child RSS does not measure host pressure. Use native APIs/files, no executable parsing or new dependencies.

| Platform | CPU | Available physical memory / total |
|---|---|---|
| Windows | `GetSystemTimes`: delta(kernel + user − idle) / delta(kernel + user); kernel already includes idle | `GlobalMemoryStatusEx`: `ullAvailPhys / ullTotalPhys` |
| macOS | `host_statistics(HOST_CPU_LOAD_INFO)`: delta(user + system + nice) / delta(all CPU ticks) | `host_statistics64(HOST_VM_INFO64)`: (free + inactive pages) × `host_page_size` / `sysctlbyname("hw.memsize")` |
| Linux | `/proc/stat` aggregate `cpu`: busy / total tick deltas; idle includes iowait; exclude guest/guest_nice already counted in user/nice | `/proc/meminfo`: `MemAvailable / MemTotal`; absent `MemAvailable` is unknown |

Existing `windows` and `libc` dependencies cover native bindings (Windows feature flags may need enabling); Linux reads use the standard library. No new crate, admin privilege or monitoring daemon.
macOS free + inactive is a reclaimable-memory approximation, not Apple's pressure metric; do not add purgeable pages again or count compressed pages as available. Document this conservative platform difference in help. Validate on real macOS before release.
Auto's macOS pressure floor reads `sysctlbyname("kern.memorystatus_vm_pressure_level")`; level 4 or higher holds (taskq.md § 3).
Reject nonpositive totals, decreasing CPU counters, impossible values or API/read failures as unknown; reset the CPU baseline. Percentages use unrounded values for decisions, rounded values only for display.

- Sample every 30 seconds using monotonic time, independently of board `poll` and while paused/held. First CPU reading establishes a baseline; no new launch until a valid interval sample exists.
- Hold immediately on a valid sample with CPU >= ceiling **or** available memory <= floor. Unknown or >60-second-old required readings also hold. Do not kill running children.
- Reopen only after three consecutive valid 30-second samples with CPU <= ceiling − 10 percentage points **and** available memory >= floor + 5. Any failed/late/mid-band sample resets the recovery count. Restart, re-enable or threshold changes start held and require this recovery.
- Allow at most one child launch per project/host per 30 seconds, counting resumes and reviewers. Recheck freshness and effective limits just before each launch; the next launch waits for a post-launch sample. This limits filling every free slot from one obsolete low reading.
- Check the gate before writing a claim; a held task stays queued without a `spawn`, retry or `gone`. If freshness/config changes after a winning claim, retain that attempt pending launch with its lease listed in the heartbeat; no replacement claim. R2/R11 and normal cancellation still apply.
- Preserve pending resumes/claims in the service's in-memory attempt handling; heartbeat lists leased pending launches as well as live children. `used` still counts only live children. This adapter/heartbeat change must ship with its contract amendment, not be hidden in a sampler.

These rules avoid threshold flapping and startup bursts. They cannot prevent a workload from spiking between samples, cannot reserve RAM for the next task, and do not coordinate different project services' launch spacing.
Non-goals: battery/thermal sensors (no portable stable signal), process throttling/termination, adaptive limits, remote config deployment, historical load graphs, or new log states.

## Contract detail

Host admission, occupancy, gate thresholds and unknown handling follow [taskq.md section3](../../taskq.md#3-attempts-lease-capacity); policy scope follows [section9](../../taskq.md#9-config-and-cutover). Runtime limits, admission.max and load-gate changes write the local installation host.json. Optional project limits only narrow it; project config never supplies missing host authority. All command surfaces use the same validated policy writer and service readback.

`taskq service capacity` uses the root command syntax, with explicit local `--host` for writes. No Fixed-mode or remote-host edit is offered by the host-policy target. Preserve untouched fields, grants, revision/concurrent-write checks and the distinction between saved and observed/applied. An invalid host policy holds admission and preserves running work. The old CLI/schema may be interpreted only under an explicitly qualified legacy profile; it cannot become fallback host authority. The [host transition cases](host-service.md#command-surface-acceptance) gate replacement and retirement.

Local CLI writes print the original observed-receipt ID and distinguish saved settings from the host's exact acknowledgment. Policy and command descriptor share one atomic host.json replacement; a same-value or stale status cannot confirm a different command. The [host command binding](host-service.md#command-surface-acceptance) defines recovery and receipt failure. Tray/remote adoption and installed acceptance remain separate gates.

A runtime whose CLI does not resolve on the service PATH admits no claim, resume or reviewer and reads as capacity reason `access` (`<cli> CLI not found on the service PATH`). On Linux every taskq process first appends an existing `~/.local/bin` to PATH, so `service install` writes it into the systemd unit and a service under an older unit still finds CLIs installed there (#1393).

Linked from `taskq.md` [§ 3](../../taskq.md#3-attempts-lease-capacity); this text binds like the contract (R8, R13).

### Slot keys

- Key: project identity + attempt. A pending grant becoming a running child keeps its one slot, counted once for runtime limits, admission.max and project caps.

### Load gate readings, host admission and emergency hold

- A missing, invalid or stale reading holds; reopening needs 3 consecutive valid samples below the band; ≤ 1 launch per 30 s. A held resume keeps its inbox; a held won claim stays leased. No battery or thermal gating, no forced retirement. Serialize slot grants and transitions; release only after verified failed launch/exit. Pick ready projects round-robin; resumes/reviewers go first.
- **Host admission.** In-process grants follow the host load gate, admission.max, runtime limits and 30 s spacing. No Fixed mode. A root whose fresh status names another live process holds admission host-wide; unknown liveness holds too. Setup (§ 4) skips limits, holds, Stop and restart.
- **Emergency hold.** `<install>/hold` blocks worker and reviewer launches (not owner sessions) before the load gate; unreadable holds. `service release` clears ordinary holds only; Mac Stop survives restart/install; selected recovery requires owner evidence and native release ([E3](emergency-recovery.md#e3-recovery)). Children continue unless owner-confirmed Full stop (R11); older services ignore it.

### Disk guard

- **Reading.** Each project service reads, before every pass and every status sample, the bytes its OS user may use on the volume of `<root>` and of `<root>/.worktrees` when that exists (a link can put it on another volume). The fullest volume decides; projects on other volumes keep running. `statvfs` (`f_bavail` × `f_frsize`) or `GetDiskFreeSpaceExW`; no executable parsing, no path in any text.
- **Rule.** Free under 20,000,000,000 bytes holds; exactly that admits. No band or cooldown: the next pass with space admits. A path that does not read makes the reading unknown, which holds. It precedes the load gate and applies with the gate off. Owner sessions, setup and external slots are not workers or reviewers and stay outside it, so an owner session can free space.
- **Projection.** `capacity.gate` is `held` (unknown reading: `unknown`) with reason `disk`. Admission code stays `load-gate` (unknown reading: `unknown`), so weighted spread skips the host under N-1 readers. Heartbeat line `disk <bytes>|unknown`; status `Hosts:` prints `disk <n> GB free` and an Anomalies row carries the reason and `Next:`; `--json` hosts carry `disk {free_bytes, holds}`. Doctor check `disk`: [health checks](host-service.md#health-checks-2194).
- **Never.** The service deletes nothing and stops nothing for space; running children continue.

## Compile jobs

Parallel workers on one host share its CPUs for Rust builds. Each cargo run defaults to one compile job per CPU, so N workers building at once start N × CPUs jobs, thrash, and keep the load gate held (taskq.md § 3).

- Each worker and reviewer launch under host admission gets `CARGO_BUILD_JOBS` = host CPUs (`available_parallelism`) / occupied host slots, this launch included, at least 1 (taskq.md § 8). Example: 16 CPUs, 4 occupied slots: 4 jobs.
- The share is fixed at launch: a worker that started alone keeps its larger share. Codex gets a new share on each resume; a Claude session keeps its first launch's share, because its settings `env` survives resume. Unknown CPU count sets nothing.
- Capacity limits, `admission.max`, `cpu_max` and `memory_min` are unchanged; the bound only spreads compile jobs.
- Codex app turns run in the app's process and receive no launch environment, so they are not bounded.

## Usage limit

A worker or reviewer whose runtime reports a usage or quota limit before any work starts ends with `usage-limit attempt <id>` (taskq.md § 2) instead of a plain `gone`:

- Detection reads the attempt log from the start. Claude: an `assistant` line with `apiError` `usage_limit_reached` or rejected `quotaLimits`, before any assistant output without an error; the reset is `resetsAt` (epoch seconds) when present. Codex: an `error` or `turn.failed` message naming a usage limit before any `item.*` event; A Codex pre-work error may omit its reset; separate structured rate-limit events can provide window/reset facts ([quota windows](#runtime-accounts)).
- The entry keeps the retry count and adds no limit card. In `doing` the task goes to `ready`; in `review` only the reviewer ends and the result stays. N-1 ignores the event and keeps the attempt live: stop old services and children and upgrade every board host before resuming (§ 9).
- A host with a spare enabled [runtime account](#runtime-accounts) moves the runtime to that account instead of holding it.
- Otherwise the service holds that runtime on this host: it has no free slot for claims, reviewers or resumes, so a task that allows another runtime starts there (taskq.md § 4 Filter). The hold ends when board time passes the reset, else the next probe 30 minutes after the limit; the next admission pass then starts the held work without an owner action.
- The hold is cached per project and rebuilt from release evidence on open tasks after restart; no local task store is added (R1).
- The heartbeat lists `limit <rt> until <time>` (reported reset) or `limit <rt> probe <time>` (next probe); `taskq status` Hosts shows ` <rt> limit until|probe <time>`, and capacity `reasons` include `usage-limit`.
- A CLI subscription limit after work started preserves its attempt/session/account and waits for verified exit and same-session continuation ([quota windows](#runtime-accounts)); it never spends a code-defect retry. Unsupported native continuation waits without replay or migration.

Release evidence records `usage-limit until <epoch>` or `usage-limit probe <epoch>` before outcome publication. Recovery reuses that deadline; the owning host also classifies its local log if an older release lacks it. Another host preserves the outcome without holding its own runtime.

## Transient model failure

A Codex worker whose turn fails on a transient model error mid-work continues in the same session instead of ending `gone` (#1843):

- Detection reads the attempt log: the last `turn.completed` or `turn.failed` record is a `turn.failed` whose message says the model is at capacity (`Selected model is at capacity. Please try a different model.`, seen on #1816 and #1818, also inside `Error running remote compact task: …`). Other errors keep the plain `gone` path; their texts are unverified (R12).
- It applies on exit without a task outcome (taskq.md § 4 step 1) to a `doing` worker on the attempt host whose log names its Codex thread. Reviewers keep `gone`.
- The service posts `taskq: steer attempt <id>` with a line that asks to continue and quotes the bounded error. The steer goes to the inbox (§ 2), and step 6 resumes the same session with it; no retry is spent.
- At most 2 such steers per attempt, counted from the board log (R1). A third failure, or an unread board log, posts `gone` as before.

## Background job at turn end

A Claude worker whose turn ends while its own background job still runs continues in the same session instead of ending `gone`. A background full check can outlive the turn, leaving an exit 0 without an outcome and wasting the attempt.

- Detection reads the Claude attempt log after the last `taskq.rules` start or resume record through AgentQ. AgentQ `Report.live_background` lists the jobs: a `Bash` or `Monitor` start whose non-error result names a native task, until a terminal notification or a stop call ends it.
- It applies on exit without a task outcome (taskq.md § 4 step 1) to a `doing` Claude worker on the attempt host. Reviewers keep `gone`.
- The service posts `taskq: steer attempt <id>` with a line that asks to continue, names the live jobs by their fixed tool label (public), never by raw command, and tells the worker to finish each in the foreground with a long Bash timeout. The steer goes to the inbox (§ 2), and step 6 resumes the same session with it; no retry is spent. A job may not survive the process end, so the worker reruns it in the foreground.
- At most 3 such steers per attempt, counted from the board log (R1). A fourth such exit, or an unread board log, posts `gone` as before.
- The worker brief tells workers to run the full check in the foreground with a long timeout and names this cause, so the steer stays a fallback.

## Runtime login

`taskq agents` (taskq.md § 5) prints, per startable runtime (`claude`, `codex`, `antigravity`; macOS/Windows `codex-app` on Codex's login), limit 0 too: binary or `not installed`, version, login, account, kind, last refresh; read-only; no login, token value or board; unreadable: `unknown` (R12); `not logged in` admits nothing.

The service probes each startable runtime's login at most every 5 minutes (taskq.md § 5 `taskq agents`):

- Claude: `claude auth status` JSON gives login, account, and kind (`authMethod` and `subscriptionType`); Claude reports no refresh time that taskq reads. Codex: `codex login status` gives login and account; `auth_mode` and `last_refresh` come from `$CODEX_HOME/auth.json` (default `~/.codex`). `codex-app` shares Codex's login. A CLI missing from the service PATH reads `not installed`.
- The probe is read-only: it never logs in and never reads or prints a token value. An unreadable field reads `unknown` (R12).
- A runtime whose last probe reads `not logged in` is held on this host: no claim, resume or reviewer, so a task that allows another runtime starts there (taskq.md § 4 Filter). The next probe that reads it logged in ends the hold; no owner action on taskq is needed. An `unknown` login admits as before, so a failing probe never stops a host.
- Capacity `reasons` include `login` (`<rt>: not logged in; <next step>`); the heartbeat lists `login <rt> off`; it carries no account. `taskq service status` prints one `Login:` line per runtime (login, account, kind, last refresh) and, for a logged-out runtime, a `Next:` line with the re-login step.
- Codex can still report logged in after its refresh token is rejected. When a Codex attempt on this host exits and its log holds Codex's own refresh-token or `401 Unauthorized` error (a `codex_*` `ERROR` line or an `error`/`turn.failed` event; worker output that quotes it does not count), the service holds Codex at once and posts `usage-limit attempt` with `Login off: codex <reason>; <next step>`, so the attempt keeps its retries. It saves `<install>/login-off/codex` with the `last_refresh` it saw; while `auth.json` keeps that `last_refresh`, the probe, `taskq agents` and `taskq service status` read `not logged in (<reason>)`. A re-login changes `last_refresh` and ends the hold. Claude's error lines are not classified yet.

The re-login steps per runtime and OS are in the [re-login runbook](../relogin.md).

## Runtime accounts

Detail of [taskq.md § 3 Runtime accounts](../../taskq.md#runtime-accounts); it binds like the contract (R8, R13).

Per CLI runtime (`claude`, `codex`, `antigravity`) each host keeps ordered, enabled/disabled login homes in `<install>/accounts.json` (none: the runtime's own `default` home). New starts take the best-ranked enabled, not usage-limited account; without fresh usage that is the first; resume, recovery and running children keep their attempt log's.
A pre-work usage limit (§ 2) fails over to the next enabled account without a retry; the § 4 hold applies only when none is left; switches are logged. `taskq accounts` lists all hosts; enable, disable, reorder and add (device login, new TaskQ-owned home) write only the local host, never copy a login or print a token; emails stay on their host, never in heartbeats. [Detail](#runtime-accounts).
Each host polls the usage of its enabled accounts and publishes `limit` heartbeat rows: an account fingerprint, percentages and resets only.
Ranking prefers `use`, then greatest remaining above a host floor (default 15%), then unknown, then below the floor; the floor never holds work and a poll never releases a hold.
`taskq accounts fleet` is the one write reaching other hosts: a trusted board entry each host applies to its own homes. A login exists per host and account in advance; switching selects a home and never moves a running attempt (R11). [Detail](#account-usage-and-fleet-policy).

**Accounts.** An account is a named login home of one CLI runtime on one host. `claude` and `codex` have accounts; `codex-app` runs in the app's process with no launch environment and always uses Codex's `default` home.

- `<install>/accounts.json` holds the owner's list per runtime, in priority order: `{"codex": [{"label": "default", "enabled": true}, {"label": "work", "enabled": true}]}`. A missing file or runtime means `[{"label": "default", "enabled": true}]`. An unreadable policy or quota record holds new starts as unknown; `taskq accounts` says why.
- A label is 1 to 32 characters of `a-z`, `0-9` and `-`, starting with a letter or digit. `default` is the runtime's own home: `CODEX_HOME` or `CLAUDE_CONFIG_DIR` of the service, else `~/.codex` or `~/.claude`. Another label's home is `<default home>/taskq-accounts/<label>`, inside the § 8 folders and outside `<install>`.
- A home starts empty. TaskQ never copies `auth.json`, credentials, config or settings between hosts or homes. A Codex account home has no `config.toml` of the owner, so Codex runs with its defaults plus the TaskQ arguments; a Claude account home holds its own workspace trust (unverified, R12). The macOS worker profile denies writes to `*.toml` in a Codex account home as in `~/.codex`.
- `<install>/accounts-state.json` is the service's own record per runtime and label: `{"limited": <board epoch>, "until": <reported reset epoch or null>}`. The limit ends at `until`, else 30 minutes after `limited` (the usage-limit probe). The record is host-wide: every project service of the host reads and writes it.

**Choice.** The active account of a runtime is the [best-ranked](#account-usage-and-fleet-policy) enabled account whose limit has ended; without fresh usage or fleet policy that is the first in list order. A new worker or reviewer start (not a resume) of `claude` or `codex` takes the active account, and writes `{"type": "taskq.account", "account": "<label>"}` to its attempt log right after the `taskq.rules` line. The child gets `CODEX_HOME` or `CLAUDE_CONFIG_DIR` of that home; `default` keeps the service's own value, including an absent variable. Login probes use the same environment as launch/resume; they never synthesize `CLAUDE_CONFIG_DIR=~/.claude` for an implicit default. A resume, a recovery, a Claude background-session observation or stop, and a transcript read use the account the attempt log names; a log without that line uses `default`. So a policy change or a failover never moves a running worker. A runtime whose accounts are all disabled admits nothing on this host: capacity and admission reason `capability` (`<rt>: every account disabled on this host`), from the fixed #1616 vocabulary.

**Quota windows (R9/R12/R13).** The record retains `limited`/`until` as the most restrictive aggregate for older readers, plus a `windows` array with observed `window`, `limited` and `until`. Independent five-hour/weekly windows remain independent; unknown type/reset stays `unknown`/null. Account polls read windows through AgentQ `account_limits`; AgentQ also classifies native errors, including Antigravity. No text inference invents a reset. Antigravity observes only its native default account; named login commands stay Claude/Codex-only.

Every project reads the host-wide record for single accounts too. Writes serialize under `accounts-state.lock`; corrupted or mismatched aggregates refuse admission. A quota observation is journaled before replacing the aggregate. A failed write fences account admission across every project until the pending observation is saved; recovery replays the journal after restart, and failed observations remain in process memory if the journal itself cannot be written. Launches require writable quota storage. Expiry permits one due probe: before launch, atomically record its attempt-log identity and a 30-minute probe hold. Other projects/accounts do not reuse that probe. A confirmed native successful completion clears only its matching probe; errors retain the observed windows, and unknown termination retains execution fencing. A quota after work records availability immediately but keeps task/attempt/session/account/work; after verified native exit a durable `steer quota attempt` resumes the same session only when its original account is eligible. For reviewers, §2.12 accepts only this service-generated quota continuation after accepted execution exited and every execution released; general review steering stays undelivered. Unavailable native continuation waits with an explanation, never replays or migrates. It never becomes a code-defect retry. No account creation, credential copy, paid API, limit/enable/permission change or migration is implied.

Heartbeat `quota <runtime> account <label> window <name> until|probe <UTC>` rows, status/PM and quota telemetry explain waits/recovery; a due timestamp says `probe due`, never confirmed healthy. The existing account-switch/fallback observations explain the selected alternative or automatic wait. Older services lack multi-window/probe fencing: coordinate host update before using the new semantics; legacy aggregate fields alone do not qualify mixed-version probes.

**Failover.** A usage limit before work ([usage limit](#usage-limit)) records the limit on the attempt's account. When the runtime has more than one account and another enabled account is active after that, the service does not hold the runtime: it posts the same `usage-limit attempt` entry (retries kept) with `; next account <label>` added to its reason, and the next admission pass starts the task on that account. When no enabled account is left, the usage-limit hold applies as before, until the earliest most-restrictive hold end of an enabled account. Release evidence rebuilds no hold for a runtime that still has an active spare account.

**Switch record.** The service computes the active account of each listed runtime every pass. When it changes, the service log gets `account <rt> <from> -> <to> (<reason>)`, where a label may be `none` and the reason is `limit` (the previous account is limited), `reset` (a limit ended) or `policy` (the owner changed the list). Telemetry gets one `account.switch` operation whose counts name `host:<host>`, `runtime:<rt>`, `from:<label>`, `to:<label>` and `reason:<reason>`. The first pass after start records the current account without a line.

**Heartbeat.** For a runtime listed in `accounts.json`, the heartbeat carries one line per account: `account <rt> <label> on|off[ active][ limited <time> until|probe <time>]`, the last limit time and its end. It never carries an email, a home path or a token.

**Commands** ([commands](commands.md#contract-detail)):

- `taskq accounts [--json]` lists every host: host, runtime, label, account email, enabled, active marker, last limit time and reset estimate (`until <time>` when reported, else `probe <time>`). This host's rows come from its own files and a read-only login probe of each home (`claude auth status`, `codex login status` with that home's variable); the email shows only when the probe names one (R12). Other hosts' rows come from their heartbeats and show `-` for the email: an email needs a run on its own host. No row prints a token or a home path.
- `taskq accounts enable|disable <runtime> <label>` and `taskq accounts move <runtime> <label> <position>` (1 is first) change the local host's list. The next start obeys it; running workers keep their login.
- `taskq accounts add <runtime> <label>` creates the empty home of a new label and runs the native device login there (`codex login --device-auth`, `claude auth login`) in the owner's terminal. On success the account goes to the end of the list, enabled. A failed login leaves the list unchanged. An existing label or a non-empty home is refused.
- The writes act on this host only and are refused under a worker or reviewer (`TASKQ_ROLE`). They never read or print a token value.

The service login probe and `taskq agents` keep reading the `default` home ([runtime login](#runtime-login)).

### Account usage and fleet policy

Detail of [taskq.md § 3 Runtime accounts](../../taskq.md#runtime-accounts) (R1, R11, R12).

**Identity.** The account fingerprint `id` is the first 8 hex digits of SHA-256 over the provider's lowercased account email or account id, read by AgentQ `account_identity` or the home's native probe. The label stays local to its host: `tight` on two hosts or `default` on two hosts prove nothing. Hosts share an account when their `id` matches. A probe that names no identity leaves the id unknown; the row is not merged with any other host. Antigravity names no email, so its id is the fingerprint of the project AgentQ reports; two accounts pinned to one project would share an id.

**Poll.** Every 120 s the host polls each enabled account of `claude`, `codex` and `antigravity` once, independent of project count, in `<install>/account-usage.json`; the calls run outside the host account lock, because a claim only tries that lock, and the lock covers the save. AgentQ `account_limits` reads each account's windows read-only, given the home a named or environment account uses; the default account keeps its native default. Native routes, credentials and response shapes are AgentQ's. Polls never refresh a token, write a login file, or send, log or print a token. A window AgentQ names without a reading, and a reason it reports without a name, are `unknown` with its reason code; the runtime's own use refreshes the login. TaskQ itself reports only `not installed` and `no login` (a named home that does not exist).

Antigravity has one `default` account per host until a per-account `agy` home is qualified; `accounts add antigravity` refuses until then, an `accounts.json` that lists another label is an error, and a fleet `use` of another account reports `policy unmet`.

Each readable window survives an unreadable sibling; the account also publishes an unknown reason naming each unreadable window. Missing or unreadable utilization never becomes zero; an absent or unreadable reset stays unknown. A repeated window name keeps each reading under a numbered name. A reason with no readable window is whole-account unknown.

A Claude poll with unreadable windows has unknown aggregate headroom for account ranking and Balancing, even while its readable windows are visible. It never clears quota holds. A later transport/auth/response failure retains the same account's incomplete evidence and its original 300-second observation expiry, never renewing it. Recovery requires a complete readable poll or fresh independent evidence after that incomplete poll expires.

Each window of a complete poll is an observation in `quota-headroom.json`, the existing store and 300 s expiry, so `Balancing` needs no new input. A partial poll's windows are not observations. A poll never clears a quota record or hold: only a reported reset or the existing probe does.

**Heartbeat.** One row per readable polled window, plus one `unknown` row per unreadable window. A whole-account failure has one whole-response `unknown` row and retains any still-fresh per-window unknowns for fleet readers. A partial Claude poll retains both kinds, with its readable windows on `limit-partial` rows. Unknown reasons use only validated names and fixed diagnostics, never provider text:

```text
limit <rt> <label> id <id|-> window <name> used <0-100> reset <UTC|-> at <UTC>
limit-partial <rt> <label> id <id|-> window <name> used <0-100> reset <UTC|-> at <UTC>
limit <rt> <label> id <id|-> unknown <reason>
```

**Reader boundary.** Binaries through v4.6.79 ignore `unknown` rows and take the smallest remaining of the `limit` windows they parse, so weekly 43 beside an unreadable Fable would read as headroom 57. A `limit` window row therefore means its poll was complete. A partial poll's readable windows use `limit-partial`, which those binaries drop without a garbled hold row. This reader shows the row and keeps its identity and expiry, but never aggregates it: any matching row marks that account incomplete. The store keeps these windows in `partial`, with `windows` empty, and writes no observation, so a binary rolled back onto the install publishes and derives nothing from a partial poll. An observation from an earlier complete poll lasts until its own 300 s expiry, as after any failed poll. No version gate applies: the boundary holds in any rollout or rollback order. DashQ and other `limit` consumers read such an account as unknown until they adopt the row.

Percent is a whole number. A changed `limit` row never forces a heartbeat write; rows travel with the periodic write. An older reader shows a usage row as a garbled limit in status and health until it updates; that is cosmetic and clears no hold. An observed account is the freshest row per runtime and `id` across this host and, on a host with an approved board, trusted fresh heartbeats of that board, so a limit used on one host ranks that account down on the others. A host with no approved board ranks on its own rows.

**Ranking.** Among enabled accounts whose limit has ended and that policy does not `avoid`, take the first group with a member:

1. The `use` account of the runtime.
2. Fresh remaining at or above the floor, greatest first. Remaining is the smallest window's remaining percent.
3. Unknown remaining, in list order.
4. Fresh remaining below the floor, greatest first.

Ties keep list order. Host.json `account_floor` is an integer 0..100, default 15; owner-only like `weights`. The floor leaves room for attempts that keep running on the account; it never refuses admission. Only the existing quota hold stops a runtime when every account is limited. The switch record reports a ranking change with reason `usage`, or `fleet` for policy.

**Fleet policy.** `taskq accounts fleet use|avoid|clear <runtime> <id>` writes `taskq: account <use|avoid|clear> <runtime> <id>` to the board's one issue labelled `taskq-accounts`; the command creates that issue when absent. The label is reserved like `taskq-host`: the issue is no task and cannot be claimed or labelled by hand. Entries fold in comment-id order per runtime and id. `use` replaces the runtime's earlier `use`; `clear` removes either. It is refused under `TASKQ_ROLE`, like every accounts write.

A host reads policy only from the board its DashQ connection approved ([speaking project](host-link.md#contract-detail)), because one account list serves all its boards. That project's pass saves the policy and other hosts' rows in `<install>/account-fleet.json` every 120 s; a save older than 600 s is ignored, so a dead project cannot pin old policy. No connection or an unreadable issue keeps the host's local ranking and says so as heartbeat line `account-policy local <reason>`. A `use` for an id the host has no home for is `policy unmet`, visible in status and as heartbeat line `account-policy unmet <rt> <id>`; `avoid` of the host's every account admits nothing, like every account disabled. `taskq accounts` shows `id`, usage, reset and the policy state per row.

**Logins.** A switch never logs in. Each account gets one native login per host in advance through `taskq accounts add` and [re-login](../relogin.md#add-an-account-to-a-host): every login holds its own token chain. Copying, brokering or relaying a login stays refused, since Codex refresh tokens are single-use and reuse kills the chain.

### Antigravity availability

`limits.antigravity` and the `--antigravity N` capacity flag count Antigravity workers, reviewers and resumes across registered projects through the same host budget; project limits only lower it. Limit zero is an owner disable, never a quota estimate. Its usage and account id come from the [poll](#account-usage-and-fleet-policy); TaskQ does not run Codex login commands against `agy`.

AgentQ supplies Antigravity quota observations through the existing `runtime::usage_limit` boundary. A pre-work quota refusal uses the existing `usage-limit` hold, reported reset or bounded next-probe time, and keeps retries. It does not change capacity or move an active attempt. Multiple provider/account quota windows and their recovery belong to [subscription quota windows](https://github.com/taskqorg/taskq/issues/2137#issuecomment-6560063367); one observed reset never proves a longer window recovered. Antigravity currently uses its native default login; TaskQ's named account login commands remain Claude/Codex only.
