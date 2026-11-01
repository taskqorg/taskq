# Capacity: limits, controls and load gating

Current host-policy authority is [taskq.md sections3/9](../../taskq.md#3-attempts-lease-capacity) and [host-service acceptance](host-service.md#command-surface-acceptance). The per-project overrides, Fixed mode, admission.json expiry and future patch below describe the superseded design; they are migration/compatibility reference, not current admission authority. Native sampling descriptions remain reference; current defaults and ownership come from the root contract.

## Legacy design reference

Design for [#783](https://github.com/alexkirs/taskq/issues/783), implemented in [#792](https://github.com/alexkirs/taskq/issues/792).
The [contract](../../taskq.md#3-attempts-lease-capacity) (§ 3, § 4, § 9) is the source of truth; where this note differs, taskq.md wins.
Follow [menu M2–M5](menu.md) and [shared density D1–D14](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md); all product text below is English.

## User model

- A slot is one live service child, worker or reviewer, of a runtime on a host **for this project**. A session or board task is not a used slot.
- Keep the existing `limits` defaults; each project's service counts only its own children. Two projects with Codex limit 4 can together run 8 on one machine. This is not a shared machine pool.
- The project has no aggregate cap; its ceiling is the sum of its hosts' effective limits. Offline/unobserved hosts do not imply zero usage.
- `used/limit` is actual children/effective ceiling: `Codex 3/4`. Zero disables new launches of that runtime; missing runtime limits remain disabled.
- Reducing 4 to 2 while 3 children run shows `3/2` until they exit. Never kill, suspend, move or replace an attempt to meet a limit (R11).
- A free slot is permission to start, not a promise. Focus, routing, dependencies and load gating must also allow it. Reviewers share the worker limit.
- Load gating does not change the number configured. `Codex 2/4 · Waiting: CPU` means two occupied slots and no new starts until recovery.
- Running children continue. A resumed session that needs a new process is a new launch: it consumes capacity and obeys the gate. Keep undelivered inbox entries pending; a held resume is not `gone` and is not marked delivered.

## Menu and CLI

In the project's existing scope/service area, add one `Capacity` disclosure, scoped to the named project and local host.
Show one row per runtime, name left and `used/limit` right; disclose `Load gate: Off`, `Ready`, `Waiting: CPU`, `Waiting: memory`, or `Unknown: <reason>` beside the controls.
Keep both reasons if both signals block; show focus/pause separately. A load hold is neither manual pause nor a failed task. Keep the existing icon semantics; do not repurpose cadence as CPU history.
No new home cards or duplicated task counts. Use the existing 16 px default, text-size setting, wrapping, keyboard focus, accessible names and touch targets; color alone cannot carry the gate state.

`Edit limits` opens inline integer fields and a `Load gate` switch with CPU ceiling and available-memory floor fields.
`Apply` first shows project, host, old → new values and `Running tasks continue`; the owner's confirmation is the write. Cancel writes nothing.
No AI session or terminal is needed. Agents may propose values, never apply them without the owner's explicit instruction; this is the proposed capacity extension of R9, not an existing authentication boundary.

Proposed CLI, run in the project's root:

```text
taskq service capacity [--host H]
taskq service capacity --host H --claude N --codex N --antigravity N
taskq service capacity --host H --load-gate on --cpu-max P --memory-min P
taskq service capacity --host H --load-gate off
```

Read defaults to the resolved local host; writes require the explicit resolved host name. Any subset of write fields changes only those fields; one command is one atomic change.
An explicit owner-issued write command supplies confirmation; the menu uses the same validation/persistence operation after its confirmation. No generic `--yes` or privileged remote service control.
Validate runtime limits as integers 0..u32::MAX; CPU ceiling 10..100 and memory floor 1..95 as integer percentages. Reject invalid combinations or unknown host names before any write.
The target host must be the local resolved host or a name in `hosts`; `TASKQ_HOST` is resolved as in §9. Never infer a host from an unrelated folder.
Preserve unknown JSON keys and untouched host/runtime entries; detect concurrent edits against the displayed/read config bytes, refuse conflicts, and replace atomically. Do not overwrite a newer owner edit.
Read back the saved values, poke the local service, then distinguish `Saved; waiting for service` from `Applied`.
The service reloads capacity before each admission decision without restarting children. Invalid edited config holds new launches and reports the error; it never silently falls back to more capacity.
`Applied` requires fresh service readback matching the requested host, effective limits and gate settings. An absent/stopped/old service can acknowledge only saved values.
Editing another host's entry saves a configuration proposal in this checkout; it does not prove deployment or application on that host. Git/config synchronization remains explicit, outside this command.

## Persistence and status

Keep top-level `limits` as existing per-project defaults on every host. Add optional `capacity_hosts`, keyed by resolved host name, with partial runtime `limits` overrides and an optional `load_gate` object.
Top-level `load_gate` supplies the per-project default; a host's `load_gate` replaces that object, then omitted fields use the fixed defaults below. No additive limit or nested partial gate merge.
Menu and CLI write the selected host entry, never change another host's ceiling by editing the defaults.

```json
{
  "limits": {"claude": 4, "codex": 4},
  "capacity_hosts": {
    "win": {
      "limits": {"codex": 2},
      "load_gate": {"enabled": true, "cpu_max": 85, "memory_min": 15}
    }
  }
}
```

Here Windows inherits Claude 4, uses Codex 2, and opts into gating; other hosts retain 4/4 and gating off.
Absent new keys preserve existing valid limits and admission behavior. Do not rewrite existing configs or enable the gate during upgrade.
Old binaries ignore these new keys: enable host overrides/gating only after the target host's compatible version is verified. A rollback to an older binary would lose enforcement; restore compatible enforcement before admitting work, or pause that host explicitly with owner approval.

Add one `capacity` object to the existing local `status.json`:

| Field | Meaning |
|---|---|
| `limits`, `used` | Effective runtime ceilings and actual live-child counts, including zero and over-limit values |
| `load_gate` | Effective `enabled`, `cpu_max`, `memory_min` settings |
| `gate` | `off`, `open`, `held`, or `unknown`; independent of focus and full slots |
| `reasons` | All blockers: `cpu`, `memory`, `sample`, `config`, or `cooldown`, with a short readable explanation |
| `cpu_percent`, `memory_available_percent` | Latest raw percentages, null when unavailable |
| `sample_at` | Wall-clock timestamp for display; null without a valid sample |
| `sample_age_seconds` | Age measured with the service's monotonic clock at status publication |

Use existing `host`, `project`, `at` and `pid` to identify/freshen the snapshot. Missing new fields mean `Unknown: service has no capacity status` ([unknown is never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy)).
Runtime rows come from the union of effective limits and live children; a removed runtime with a child still shows usage and limit 0.
Update the capacity snapshot every sample and after launch/reap/reload, without forcing a board poll every sample. Mark a sample/status older than 60 seconds unknown; add elapsed display time to published age, and treat clock anomalies as unknown.
The tray reads only registered local project snapshots, no board/token. Remote hosts show `Unknown: no local snapshot`; do not invent fleet usage from heartbeat task counts. A future remote transport is separate work.

## Load model

Recommend CPU busy percentage across all logical processors and physical memory available percentage. Both reflect all host activity, including other projects and unrelated programs.
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

- Disabled by default on old and new configs. When the owner enables it, suggested defaults are CPU ceiling 85% and available-memory floor 15%; these reserve headroom, not a hardware safety guarantee.
- Sample every 30 seconds using monotonic time, independently of board `poll` and while paused/held. First CPU reading establishes a baseline; no new launch until a valid interval sample exists.
- Hold immediately on a valid sample with CPU >= ceiling **or** available memory <= floor. Unknown or >60-second-old required readings also hold. Do not kill running children.
- Reopen only after three consecutive valid 30-second samples with CPU <= ceiling − 10 percentage points **and** available memory >= floor + 5. Any failed/late/mid-band sample resets the recovery count. Restart, re-enable or threshold changes start held and require this recovery.
- With gating enabled, allow at most one child launch per project/host per 30 seconds, counting resumes and reviewers. Recheck freshness and effective limits just before each launch; the next launch waits for a post-launch sample. This limits filling every free slot from one obsolete low reading.
- Check the gate before writing a claim; a held task stays queued without a `spawn`, retry or `gone`. If freshness/config changes after a winning claim, retain that attempt pending launch with its lease listed in the heartbeat; no replacement claim. R2/R11 and normal cancellation still apply.
- Preserve pending resumes/claims in the service's in-memory attempt handling; heartbeat lists leased pending launches as well as live children. `used` still counts only live children. This adapter/heartbeat change must ship with its contract amendment, not be hidden in a sampler.

These rules avoid threshold flapping and startup bursts. They cannot prevent a workload from spiking between samples, cannot reserve RAM for the next task, and do not coordinate different project services' launch spacing.
Non-goals: battery/thermal sensors (no portable stable signal), process throttling/termination, adaptive limits, auto-enabling gates, machine-wide quotas, shared fairness, remote config deployment, historical load graphs, or new log states.

## Proposed taskq.md patch

This is a **future** contract patch, not an amendment applied by this PR. It replaces existing lines and adds four: 296 → 300 on the reviewed base.
The compact contract sets the decisions; this note supplies rationale and examples. R9 gains an explicit capacity confirmation rule; R1/R2/R11 remain unchanged.

```diff
@@ §3 Capacity @@
-- **Capacity** is per host: `limits.<runtime>` minus the service's live children of that runtime. No project cap: the
-  sum of host limits bounds the project.
+- **Capacity** is per project/host: effective runtime limits minus all its live children, reviewers/resumes included; zero disables launches. No project cap or shared host pool. Lowering a limit never stops children.
+- **Load gate** is opt-in: CPU busy >= `cpu_max` or available physical memory <= `memory_min` holds all new child launches; existing children continue. Windows reads GetSystemTimes/GlobalMemoryStatusEx, macOS host CPU/VM statistics (free + inactive)/hw.memsize, Linux /proc/stat (idle includes iowait, excludes duplicate guest ticks)/meminfo MemAvailable; missing/invalid readings hold.
+- Sample on monotonic 30-second ticks, independent of board poll; startup/config changes hold until three consecutive valid samples at CPU <= ceiling − 10 and memory >= floor + 5. Failed/late/mid-band samples reset recovery; readings older than 60 seconds hold. At most one launch per project/host per 30 seconds, after a post-launch sample; check before claim and launch.
+- Held resumes keep inbox pending; a won claim held before launch stays leased in the heartbeat, not counted used, without another claim/retry/gone. Restart follows §4 recovery. No battery/thermal gating or forced retirement.
@@ §4 Heartbeat: replace the attempt-list line @@
-   and `#<N> <id>` per live child, plus `at <board time>` (a same-body edit keeps `updated_at`). Freshness uses the board's `updated_at` and the response `Date`, never local clocks.
+   and `#<N> <id>` per live child or leased pending launch, plus `at <board time>` (a same-body edit keeps `updated_at`). Freshness uses the board's `updated_at` and the response `Date`, never local clocks.
@@ §4 Poke/status @@
-  writes `<root>/.taskq/status.json` (state, open tasks with labels, `request`, `update`: a newer version the last check found and did not install, else null, `events_since`) for `service status` and the tray.
+  writes `<root>/.taskq/status.json` (state, open tasks with labels, `request`, `update`: a newer version the last check found and did not install, else null, `events_since`) for `service status` and the tray; `capacity` adds effective `limits`, live-child `used`, effective `load_gate`, `gate` (off/open/held/unknown), all `reasons`, CPU/available-memory percentages, `sample_at`, monotonic `sample_age_seconds`; unknown readings are null, missing/stale (>60 s) capacity is unknown. Publish capacity on sample/launch/reap/reload; remote usage needs a local snapshot, else it is unknown. Reload capacity before admission; bad config holds launches, children continue.
@@ §5 Commands: insert before selftest @@
+| `taskq service capacity [--host H] [--claude N] [--codex N] [--antigravity N] [--load-gate on\|off] [--cpu-max P] [--memory-min P]` | read: local host default; write: explicit known host, owner instruction (menu confirms named old/new values first, R9). Same atomic JSON-preserving operation; detect concurrent edits; read back saved config, poke locally, say applied only after fresh matching effective service readback. Other hosts: saved only, no remote deployment. |
@@ §9 Config: insert after hosts/has/limits @@
+- `capacity_hosts` optional resolved-host map: partial `limits` override existing per-project defaults, absent runtime = disabled; optional host `load_gate` replaces the top-level default object. Gate fields: `enabled` false, `cpu_max` 85, `memory_min` 15 when omitted. Validate limits 0..u32::MAX, CPU 10..100, memory 1..95 (integer percentages); preserve other keys. Absent new keys preserve behavior; no auto-migration/enabling. Use new fields only on verified compatible hosts; older rollback loses enforcement and needs compatible restoration or owner-approved pause before admission.
```

## Owner questions before implementation

These choices do not block this proposal; none is applied by a design-only merge. Ask on the implementation task before changing a decision.

1. Keep per-project/host ceilings, or introduce a shared machine pool? Recommend **keep existing scope plus named host overrides**; a shared pool requires separate coordination and migration work.
2. Opt-in gate with CPU 85%, available memory 15%, 30-second samples and 90-second recovery, or enable automatically? Recommend **opt-in with these conservative starting values**, preserving every existing host's behavior.
3. Hold on unknown readings and gate resumes/reviewers, or exempt them/fail open? Recommend **hold every new process**, with pending inbox/claims leased and visible; it may delay answers but respects the ceiling and protects headroom.
4. Confirm in-menu and accept explicit owner CLI writes, or require a separate confirmation command for CLI too? Recommend **menu confirmation plus explicit owner CLI instruction**, with identical validation and readback; local file permissions remain the security boundary.

## Implementation review checklist (not qualification)

- Scope: config parsing/reload and capacity command in `src/main.rs`; admission, claim/resume and heartbeat handling in `src/service.rs`; native sampling in `src/platform.rs`; tray state/action bridge and its UI; help, fixtures and contract. No change to `fold` or task states is proposed.
- Verify unchanged legacy configs, partial host overrides, absent runtimes, 0/full/over-limit counts, workers plus reviewers/resumes, lowering limits, config conflicts/invalid edits, unknown remote/stale/old-service snapshots, saved vs applied and rollback compatibility.
- Use deterministic tick/reading tests for startup, exact thresholds, mid-band recovery resets, stale readings, both blockers, post-launch spacing and pending claims/resumes; native tests must compare CPU and memory readings on Windows/macOS/Linux. Model tests alone do not qualify native sensors (R12).
- Menu acceptance: 320/360 px, long names, 14–20 px, 200% scale, both themes, keyboard/screen reader/touch, cancel, changed-config confirmation and readback errors. No render or sensor qualification is claimed by this note.

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

## Compile jobs

Parallel workers on one host share its CPUs for Rust builds. Each cargo run defaults to one compile job per CPU, so N workers building at once start N × CPUs jobs, thrash, and keep the load gate held (taskq.md § 3).

- Each worker and reviewer launch under host admission gets `CARGO_BUILD_JOBS` = host CPUs (`available_parallelism`) / occupied host slots, this launch included, at least 1 (taskq.md § 8). Example: 16 CPUs, 4 occupied slots: 4 jobs.
- The share is fixed at launch: a worker that started alone keeps its larger share. Codex gets a new share on each resume; a Claude session keeps its first launch's share, because its settings `env` survives resume. Unknown CPU count sets nothing.
- Capacity limits, `admission.max`, `cpu_max` and `memory_min` are unchanged; the bound only spreads compile jobs.
- Codex app turns run in the app's process and receive no launch environment, so they are not bounded.

## Usage limit

A worker or reviewer whose runtime reports a usage or quota limit before any work starts ends with `usage-limit attempt <id>` (taskq.md § 2) instead of a plain `gone`:

- Detection reads the attempt log from the start. Claude: an `assistant` line with `apiError` `usage_limit_reached` or rejected `quotaLimits`, before any assistant output without an error; the reset is `resetsAt` (epoch seconds) when present. Codex: an `error` or `turn.failed` message naming a usage limit before any `item.*` event; Codex reports no reset that taskq reads.
- The entry keeps the retry count and adds no limit card. In `doing` the task goes to `ready`; in `review` only the reviewer ends and the result stays. N-1 ignores the event and keeps the attempt live: stop old services and children and upgrade every board host before resuming (§ 9).
- A host with a spare enabled [runtime account](#runtime-accounts) moves the runtime to that account instead of holding it.
- Otherwise the service holds that runtime on this host: it has no free slot for claims, reviewers or resumes, so a task that allows another runtime starts there (taskq.md § 4 Filter). The hold ends when board time passes the reset, else the next probe 30 minutes after the limit; the next admission pass then starts the held work without an owner action.
- The hold is cached per project and rebuilt from release evidence on open tasks after restart; no local task store is added (R1).
- The heartbeat lists `limit <rt> until <time>` (reported reset) or `limit <rt> probe <time>` (next probe); `taskq status` Hosts shows ` <rt> limit until|probe <time>`, and capacity `reasons` include `usage-limit`.
- A limit reported after work started keeps the plain `gone` or `failed` path and counts as a retry.

Release evidence records `usage-limit until <epoch>` or `usage-limit probe <epoch>` before outcome publication. Recovery reuses that deadline; the owning host also classifies its local log if an older release lacks it. Another host preserves the outcome without holding its own runtime.

## Transient model failure

A Codex worker whose turn fails on a transient model error mid-work continues in the same session instead of ending `gone` (#1843):

- Detection reads the attempt log: the last `turn.completed` or `turn.failed` record is a `turn.failed` whose message says the model is at capacity (`Selected model is at capacity. Please try a different model.`, seen on #1816 and #1818 on 2026-10-29, also inside `Error running remote compact task: …`). Other errors keep the plain `gone` path; their texts are unverified (R12).
- It applies on exit without a task outcome (taskq.md § 4 step 1) to a `doing` worker on the attempt host whose log names its Codex thread. Reviewers keep `gone`.
- The service posts `taskq: steer attempt <id>` with a line that asks to continue and quotes the bounded error. The steer goes to the inbox (§ 2), and step 6 resumes the same session with it; no retry is spent.
- At most 2 such steers per attempt, counted from the board log (R1). A third failure, or an unread board log, posts `gone` as before.

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

**Accounts.** An account is a named login home of one CLI runtime on one host. `claude` and `codex` have accounts; `codex-app` runs in the app's process with no launch environment and always uses Codex's `default` home.

- `<install>/accounts.json` holds the owner's list per runtime, in priority order: `{"codex": [{"label": "default", "enabled": true}, {"label": "work", "enabled": true}]}`. A missing file or runtime means `[{"label": "default", "enabled": true}]`. An unreadable policy or quota record holds new starts as unknown; `taskq accounts` says why.
- A label is 1 to 32 characters of `a-z`, `0-9` and `-`, starting with a letter or digit. `default` is the runtime's own home: `CODEX_HOME` or `CLAUDE_CONFIG_DIR` of the service, else `~/.codex` or `~/.claude`. Another label's home is `<default home>/taskq-accounts/<label>`, inside the § 8 folders and outside `<install>`.
- A home starts empty. TaskQ never copies `auth.json`, credentials, config or settings between hosts or homes. A Codex account home has no `config.toml` of the owner, so Codex runs with its defaults plus the TaskQ arguments; a Claude account home holds its own workspace trust (unverified, R12). The macOS worker profile denies writes to `*.toml` in a Codex account home as in `~/.codex`.
- `<install>/accounts-state.json` is the service's own record per runtime and label: `{"limited": <board epoch>, "until": <reported reset epoch or null>}`. The limit ends at `until`, else 30 minutes after `limited` (the usage-limit probe). The record is host-wide: every project service of the host reads and writes it.

**Choice.** The active account of a runtime is the first enabled account whose limit has ended. A new worker or reviewer start (not a resume) of `claude` or `codex` takes the active account, and writes `{"type": "taskq.account", "account": "<label>"}` to its attempt log right after the `taskq.rules` line. The child gets `CODEX_HOME` or `CLAUDE_CONFIG_DIR` of that home; `default` keeps the service's own value. A resume, a recovery, a Claude background-session observation or stop, and a transcript read use the account the attempt log names; a log without that line uses `default`. So a policy change or a failover never moves a running worker. A runtime whose accounts are all disabled admits nothing on this host: capacity and admission reason `capability` (`<rt>: every account disabled on this host`), from the fixed #1616 vocabulary.

**Quota windows (R9/R12/R13).** The record retains `limited`/`until` as the most restrictive aggregate for older readers, plus a `windows` array with observed `window`, `limited` and `until`. Independent five-hour/weekly windows remain independent; unknown type/reset stays `unknown`/null. Claude structured rate-limit metadata and Codex structured window facts are read at the existing runtime boundary; AgentQ still classifies native errors, including Antigravity. No text inference invents a reset. Antigravity observes only its native default account; named login commands stay Claude/Codex-only.

Every project reads the host-wide record for single accounts too. Writes serialize under `accounts-state.lock`; corrupted or mismatched aggregates refuse admission. Expiry permits one due probe: before launch, atomically record its attempt-log identity and a 30-minute probe hold. Other projects/accounts do not reuse that probe. A confirmed native successful completion clears only its matching probe; errors retain the observed windows, and unknown termination retains execution fencing. A quota after work records availability immediately but keeps task/attempt/session/account/work; after verified native exit a durable `steer quota attempt` resumes the same session only when its original account is eligible. For reviewers, §2.12 accepts only this service-generated quota continuation after accepted execution exited and every execution released; general review steering stays undelivered. Unavailable native continuation waits with an explanation, never replays or migrates. It never becomes a code-defect retry. No account creation, credential copy, paid API, limit/enable/permission change or migration is implied.

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

### Antigravity availability

`limits.antigravity` and the `--antigravity N` capacity flag count Antigravity workers, reviewers and resumes across registered projects through the same host budget; project limits only lower it. Limit zero is an owner disable, never a quota estimate. Native `agy` sign-in/account probing is unqualified and reports unknown; TaskQ does not run Codex login commands against it.

AgentQ supplies Antigravity quota observations through the existing `runtime::usage_limit` boundary. A pre-work quota refusal uses the existing `usage-limit` hold, reported reset or bounded next-probe time, and keeps retries. It does not change capacity or move an active attempt. Multiple provider/account quota windows and their recovery belong to [subscription quota windows](https://github.com/taskqorg/taskq/issues/2137#issuecomment-6560063367); one observed reset never proves a longer window recovered. Antigravity currently uses its native default login; TaskQ's named account login commands remain Claude/Codex only.
