# One service process per host

Status: design detail for the adopted host model in [taskq.md](../../taskq.md). Runtime recovery follows [execution ownership](../runtime-qualification.md#execution-ownership); remaining migration and native qualification are not implied by this document.

## Decision

- One unit per host: `com.taskq.service` (launchd), `TaskQ` (Task Scheduler), `taskq.service` (systemd user). It runs `<install>/taskq service run` with `<install>` as working directory.
- Startup owns the existing CoreQ native lock at `<install>/service.lock` for the whole run. PID publication is a projection; it cannot serialize simultaneous startup. Keep the lock through cleanup and every project thread; restart reconciliation never treats record age as exit.
- The process serves every root listed in `<install>/projects`, one project thread each.
- Host-wide parts run once, in process: admission, the GitHub request budget and rate-limit pause, the update check and re-exec, the tray spawner, the profile appender, hourly request records and bounded log pruning.
- Per-project parts stay per project: `taskq.json`, `<root>/.taskq/` (`status.json`, `events`, `focus`, `focus.paused`, legacy `kids`, `poke`, `update`, `incidents/`, attempt logs, `cli-errors.log`, `pm-cursor`, specq cache), worktrees, the project log `<install>/<project>.log` and one heartbeat issue per board.
- Native recovery records stay under protected `<install>/native/`, keyed only by canonical registered project roots; [handover contract](../runtime-qualification.md#adapter-recovery-matrix).
- `service install` and `service uninstall` in a project edit `<install>/projects` and poke the process; they install or remove the host unit only when it is missing or no project remains.

## Historical design map

Original old-to-target proposal, retained as design rationale. The adopted taskq.md contract, current linked owner pages and H-A1–H-A5 below govern implementation; these historical quotations are not independent active rules and include superseded telemetry wording. R8 retains the 300-line/60 KiB root bound. No board fold or golden trace change belongs to this transition.

### § 3 Host admission (line 144)

Replace:

> - **Host admission.** `<install>/admission.json` under `<install>/admission.lock` admits every Auto launch after the load gate; a grant needs every fresh service of `<install>/projects` at `4.2.0-dev` or newer, held reservations under `admission.max` and the 30 s launch spacing. An unreadable file refuses; Fixed services are never refused.

With:

> - **Host admission.** The host service admits every launch across projects after the host load gate, in process: held reservations under admission.max, runtime limits and the 30 s launch spacing. Fixed mode is removed. Until all legacy services have drained and exited, hold new admission host-wide; existing attempts keep leases and may resume to finish. Unknown legacy liveness holds admission.

### § 4 opening (line 148)

Replace:

> `taskq service run` is the host's single long-lived process; `taskq service install` makes the OS keep it running (§ 8).

With:

> `taskq service run` is the host's single long-lived process for every root in `<install>/projects`, one project thread each running the loop below; `service install` registers a project and makes the OS keep the process running (§ 8). A panic in a project thread restarts that project only: its children keep running, re-attached to the rebuilt state; three panics in 10 minutes quarantine it (reap, heartbeat, `status.json` only) until `service restart`.

### § 4 step 7 (line 158)

Replace:

> 7. Every `update_every` (60 s), or at once when a local command wrote `<root>/.taskq/update`: the self-update check (§ 8); an installed service applies a newer version, others only report it. Its outcome is logged.

With:

> 7. Once per host every `update_every` (60 s), or at once when a local command wrote any `<root>/.taskq/update`: the self-update check (§ 8); an installed service applies a newer version, others only report it. Its outcome is logged in every project log.

### § 4 Poke (line 164, first sentence)

Replace:

> - Poke: a local command touches `<root>/.taskq/poke`; the sleeping service checks its mtime every second. Each pass

With:

> - Poke: a local command touches `<root>/.taskq/poke` (that project) or `<install>/poke` (the project list); the service checks their mtimes every second. Each pass

### § 4 One service (line 167)

Replace:

> - One service per root and host name: `service run` waits while a `status.json` under 60 s old names another live pid; a pass that finds this host's heartbeat under `ttl` naming another `service <id>` writes nothing and fails with `another service <id> runs as host <name>`. Restart: rebuild from the board and reconcile original native executions; missing handles stay unknown and never imply `gone` or release. `service restart` writes `<root>/.taskq/restart`: the next pass re-execs (§ 8 step 3).

With:

> - One service per host: `service run` waits while `<install>/service` (the service's pid) is under 60 s old and names another live pid; a project thread waits while its `status.json` under 60 s old names another live pid; a pass that finds this host's heartbeat under `ttl` naming another `service <id>` writes nothing and fails with `another service <id> runs as host <name>`. Restart: rebuild from the board and reconcile original native executions; missing handles stay unknown and never imply `gone` or release. `service restart` writes `<install>/restart` (a project's `<root>/.taskq/restart` counts the same): the process re-execs (§ 8 step 3).

### § 4 Tray (line 168, first sentence)

Replace:

> - Tray (macOS, Windows; [menu](menu.md#contract-detail) M1–M14): one per host, started by any installed service when `<install>/tray` (its pid) is older than 30 s.

With:

> - Tray (macOS, Windows; [menu](menu.md#contract-detail) M1–M14): one per host, started by the installed service when `<install>/tray` (its pid) is older than 30 s; restart and stop act on the host service and name every project first; pause, resume and uninstall stay per project.

### § 4 Profiling (line 177, first sentence)

Replace:

> Records hold fixed codes and numbers only in `<install>/taskq.log` under its § 4 Data bound, appended under the nonblocking `taskq-profile.lock`;

With:

> Records hold fixed codes and numbers only in `<install>/taskq.log` under its § 4 Data bound; the service appends through one in-process writer, every writer under `taskq-profile.lock`;

### § 5 service row (line 196)

Replace the `Does` cell:

> § 4, § 8; `permissions`: § 8 Access read, or one grant write with readback; `hold`/`release`/`recover`: § 3, § 4 emergency; `stop`: until the next logon, the unit stays; `list`: this host's projects and their focus; `tray --snapshot DIR [--route R,..]`: § 4

With:

> § 4, § 8; `install`/`uninstall [--host]`: register or drop this project in `<install>/projects` and poke the host service (`--host`: drain, verify exit, then remove only the host unit, § 8 rollback); `permissions`: § 8 Access read, or one grant write with readback; `hold`/`release`/`recover`: § 3, § 4 emergency; `stop`/`restart`: the host service, every project (`stop` until the next logon, the unit stays); `status`: the process, then one block per project; `list`: this host's projects and their focus; `tray --snapshot DIR [--route R,..]`: § 4

### § 8 Shared core (line 266, unit names)

Replace:

> units `com.taskq.<project>`, `TaskQ-<project>`, `taskq-<project>.service`.

With:

> one host unit `com.taskq.service`, `TaskQ`, `taskq.service` ([host service](host-service.md#migration) removes the legacy `com.taskq.<project>`, `TaskQ-<project>`, `taskq-<project>.service`).

### § 8 step 3 (line 269)

Replace:

> 3. At step 7 an installed service older than `current` re-execs through the launcher; live CLI children and Claude background turns with a saved native identity pass over through the protected native handover record; an active codex-app or operator turn defers it. Detail: [runtime qualification](../runtime-qualification.md#contract-detail).

With:

> 3. At step 7 an installed service older than `current` re-execs through the launcher, once for all projects; on Unix, live CLI children and Claude background turns with a saved native identity pass over through each project's protected native handover record with the same pid; an active codex-app or operator turn of any project defers it. On Windows, all projects must have no live child or active turn before exit 75; the unit restarts with a new pid, with no child adoption. Detail: [runtime qualification](../runtime-qualification.md#contract-detail).

### § 8 step 5 (line 271)

Replace:

> 5. `service install` installs its binary as a new file, never over an existing release binary (macOS kills rewritten binaries). While the project's `status.json` lists live children (or is unreadable) it writes `<root>/.taskq/restart` instead of reloading. Windows stops end the process tree (`taskkill /T`); run Windows setup outside an MSIX app. The coreq launcher counts unhealthy starts per project.

With:

> 5. `service install` installs its binary as a new file, never over an existing release binary (macOS kills rewritten binaries), adds the root to `<install>/projects` under `<install>/projects.lock`, writes and loads the host unit when missing or changed, and pokes `<install>/poke`; `installed` only after the project's `status.json` names the service pid. While any project's `status.json` lists live children (or is unreadable) a changed unit writes `<install>/restart` instead of reloading. `uninstall` drops the root, admits nothing more there and keeps its children until they exit (R11); with no project left it removes the unit only after all draining children/turns have ended and process exit is verified. Windows stops end the process tree (`taskkill /T`); run Windows setup outside an MSIX app. The coreq launcher counts unhealthy starts of the host unit and never rolls back below the first host-service version.

### § 9 timers (line 286)

Replace:

> - `ttl`, `poll`, `update_every`, `rollback_after`: seconds, defaults 300, 60, 60, 600.

With:

> - `ttl`, `poll`, `update_every`, `rollback_after`: seconds, defaults 300, 60, 60, 600; `ttl` and `poll` per project, host.json owns `update_every` and `rollback_after`, read once per pass.

### § 3 Capacity and Load gate replacements

Replace the Capacity bullet with:

> - **Capacity** is host-wide: `<install>/host.json` runtime limits minus occupied slots across every registered project, including reviewers and resumes. One slot is keyed by project identity plus attempt: a granted pending launch and its running child are the same occupancy, counted once for host runtime limits, admission.max and project caps. The pending-to-running transition never adds another slot. An optional project `limits` can only lower that project's share: effective remaining capacity is the minimum of host remaining and project remaining. Missing runtime limit means no count ceiling; 0 disables. Lowering never stops children (R11). The daemon reads and validates host.json once per pass; invalid or unreadable policy holds all new launches and reports unknown, without discarding existing children or leases.

Replace `Load gate (opt-in, § 9)` with `Load gate (host policy, § 9)`; retain its thresholds, unknown handling, three-sample reopening and 30 s spacing. Remove every Fixed-mode bypass. Serialize slot grants and pending-to-running transitions; retain the same slot until verified failed launch/exit, then release it. Count the union of pending grants and running attempts by project identity plus attempt, never their sum or issue number alone; rebuilding from child handles preserves that identity. Choose ready projects round-robin, preserving priority then issue order within each project; resumes/reviewers retain their existing precedence. Count legacy work during drain; migration admits no new tasks.

### § 5 capacity command replacement

> Use the canonical `taskq service capacity` syntax in taskq.md section5; `--host` is explicit and local for writes. No --mode or remote-host edit. Optional project limits remain narrowing project configuration; this target adds no unimplemented --project command.

> Read or edit this host's host.json through the same policy operation regardless of invocation directory. Show host limits/use and narrowing project caps separately, write atomically on explicit owner instruction (R9), poke and report applied only after daemon readback. Preserve resource-grant checks and revision/receipt guards; no new authority for workers. Legacy per-project edits remain a qualified compatibility path only until retirement.

### § 9 host and project configuration replacement

Replace the `hosts`/`has`/`limits`, `capacity_hosts`/`admission`, `permission_mode`/`codex`, and timers bullets with these four bullets:

> - Host-owned `<install>/host.json`: `hosts` hostname-to-name mapping (TASKQ_HOST override), `has` name-to-capabilities mapping, runtime `limits`, `admission` {"max": n}, `load_gate`, `codex` args, `permission_mode` (R9) and `claim` `any|labelled`. One local host policy, no capacity_hosts or mode. Host admission and load-gate defaults retain the existing Auto/D03 defaults; migration never silently enables a gate or changes runtime permissions.
> - Project taskq.json retains board/repo/board host, publish/delivery, workspace, labels, recipes, incident_reports, pages, assignee and all other project-only keys; optional `limits` only lowers the host limits. No host identity, capability, model/permission or admission copies remain here. Token lookup and worker-policy grants (§ 8 Access) are unchanged; host flags never grant access.
> - `ttl` and `poll` stay per project (300 and 60 s); `update_every` and `rollback_after` move to host.json (60 and 600 s). Missing/invalid host policy reads unknown and holds launches; it never falls back to per-project copies.
> - `<install>/projects` remains the explicit owner-approved list of exact roots; no directory discovery (R10). Install/uninstall edits it under projects.lock, atomically, and pokes the daemon. Removing a project drains its existing attempts before retiring its thread or removing the last host unit; status retains a draining row until verified exit.

Update the § 4 update interval replacement to read host.json timers, rather than aggregating project timers. Update § 8 step 4: normal Unix image updates preserve eligible CLI children; Windows waits for idle across all projects. Topology migration and reverse rollback use the explicitly approved drain below. Keep releases immutable, checksum/smoke verification and atomic current/previous activation. Before activation, preserve the old launcher, unit definitions and configuration; set the topology rollback floor only after verified host-unit ownership. Failure before that boundary restores the legacy state, not a partial host installation.

### Six-copy configuration migration

The six existing project `capacity_hosts` copies are input, not six independent host policies. On each host, read only roots already in projects; record six expected inputs as present/absent/unknown without discovering folders. Resolve each current effective local-host policy using existing override precedence, including top-level defaults, host mappings, capabilities, codex args, permission mode and timers. Do not copy another host's capacity entry into the local policy.

Compare the resolved policies before writing anything. Identical values collapse to one host.json. Differences, missing/unreadable inputs, Fixed-to-Auto changes, and policies that cannot be represented as host limits plus lowering project caps are owner choices: show values and affected projects, recommend a conservative policy, and stop migration until answered. Never infer that a zero in one copy is a host-wide owner decision. This design does not assert those six live copies were inspected in this task.

After approval, save byte-for-byte originals and the chosen mapping in a bounded migration backup under install; preserve unknown keys and original file ownership. Stage host.json and project edits, validate all, then atomically replace each file with restart-safe progress/readback. Keep every affected service admission-held until the whole set reads back; interruption resumes or restores originals, never enables partial policy. Remove only migrated keys, including capacity_hosts, admission/mode, load_gate, hosts, has, codex and permission_mode; keep optional lowering limits and unrelated keys. Preserve per-host backups: project files may contain remote-host copies needed by another host's migration. Re-run is idempotent. Do not commit local paths or secrets. Rollback restores these originals only within a qualified compatible protocol version; legacy services that expire attempts cannot restart against unresolved executions.

## Migration

**Retired.** Every host finished this migration; legacy mode, the native legacy inventory and the telemetry budget bridge are removed. Disabled per-project unit files that remain on a host are dead: no current service reads them. The steps below describe the finished one-time handoff; the live service follows only the one-service status guard (step 3) and the newer-build attach.

Owner approval of this drain handoff is required before implementation (R13); it replaces the no-drain promise only for the topology migration and its reverse. The first host-service release is N. Legacy services run N-1 or older. After approval of the design and resolved configuration mapping, migration needs no additional owner step on an automatically updating host; it runs once per host, idempotent, in any order across hosts.

1. **Update.** A legacy service applies N through the existing update (§ 8 step 2) and re-execs at step 7 (§ 8 step 3). On Unix, eligible live CLI children and Claude background turns pass over through the protected native handover record with the same pid; active codex-app and operator turns defer it. Windows waits for every child and active turn to finish, exits 75, and the legacy unit starts N with a new pid; Windows does not adopt children.
2. **Legacy mode.** N started by a per-project unit (working directory is a project root, unit name `com.taskq.<project>`, `TaskQ-<project>` or `taskq-<project>.service`) runs that project in legacy mode:
   - It adds the root to `<install>/projects` if missing, then writes and loads the host unit if missing.
   - It admits nothing new: capacity reason `handoff`. It keeps reaping, resuming answers of its own live attempts, beating and writing `status.json`, so no lease expires.
   - When it has no live child, it removes its own unit (`launchctl bootout` and plist delete; `schtasks /Delete`; `systemctl --user disable` and unit file delete) and exits 0. Run removal through an idle handoff helper where unloading kills the caller; the helper verifies daemon exit and unit absence. A unit removal it cannot read back stays in the project log and in `status.json` `reason`; the process then keeps running in legacy mode and retries every pass.
3. **Host service.** The host unit starts N in host mode. Each project thread waits while that project's fresh `status.json` names another live pid (§ 4 one service), so a legacy or N-1 service and the host service never serve one root together. When the legacy process exits, the thread rebuilds from the board; admission by focus starts only after all legacy roots have drained and host policy migration reads back.
4. **N-1 on the same host.** An N-1 legacy service that has not updated yet keeps its unit and its root; the host thread for that root waits. New admission is held host-wide until all legacy services drain and exit. Legacy Fixed services cannot be made safe merely by joining the old Auto protocol; no legacy service is force-stopped.
5. **Readback.** `service status` shows `Service: running (host, pid <p>)` and one block per project. Migration is complete on a host when no legacy unit file exists and every registered root's `status.json` names the host service pid. The release qualification task records this per host.

**Newer build under a running host** (#1306): `service install` from a binary newer than the running host first writes the attach intent to `<install>/projects.upgrading`, never to `projects.installing`, so a host that predates this step keeps its transitions and holds no launches. After the `received` receipt the candidate becomes `current` (previous: the running release; rollback floor kept, else set to the running release) and `<install>/update` pokes the host's own update step, which re-execs once its [handover](#restart-handover) waits clear (step 1) and logs `current is <v>; restarting`; no `restart` marker is written. The caller waits up to 300 s for the host to report the candidate. The attach then adopts the intent's command and receipt as its `projects.installing` cursor, removes the intent and runs to the observed receipt. On timeout the caller fails, the intent stays, and a repeated install for the same root continues the same command. A host that runs the candidate replaces the queued receipt's handover reason at once with `upgrade <id> runs <v>; the waiting service install in <root> finishes it`, retrying until retained (#1402). After 300 s while the intent stays queued it logs `upgrade <id> already runs <v>; repeat service install in <root> to finish it` once per generation and sets it as the receipt reason (#1357).

Why drain instead of cross-process adoption: removing a legacy unit kills its process tree on systemd (`KillMode=control-group`) and may on launchd; Windows has no adoption today. Children therefore stay with the process that started them until they exit; no attempt goes `gone` and no worker runs twice. The cost is that new admission waits host-wide for the longest legacy attempt and any unresolved configuration decision; no finite completion time is promised. Cross-process adoption on macOS is an upgrade path only after a probe shows a child in its own process group survives `launchctl bootout` (R12).

### Rollback

- **Within host-service versions** (N+k to N): the coreq launcher rolls back after three unhealthy starts of the host unit, as today per project, and never below N (rollback floor, like § 8 Access).
- **Back to per-project units** (owner decision only):
  1. `taskq service uninstall --host`: requests a host-wide drain, keeps `<install>/projects`, and admits no new attempts. Reaping, heartbeats and inbox delivery for existing attempts continue until all children and active turns end. Keep the host unit installed throughout the drain; unknown liveness blocks removal.
  2. Read back zero live children/turns, stop the idle host daemon and verify its recorded pid has exited; only then unload/delete the host unit and verify removal. Never remove a unit while it owns live children. A failed readback stops rollback before any older unit is installed.
  3. Restore the saved per-project configuration and prior launcher/current state, including an explicit update hold so N is not immediately reapplied; then run the preserved older release's `service install` in each registered root. Verify each legacy unit and its new per-project pid in `status.json` before releasing the hold. Keep originals and host.json for retry; do not delete worker worktrees or board history.
- An N-1 binary never starts as a host service: the host unit runs `<install>/taskq service run` without a project root, which N-1 refuses at config load. The rollback floor keeps the launcher from picking N-1.

## Isolation

- **Project panic.** Each project runs in its own thread. A pass runs inside `catch_unwind`. On a panic:
  - The project's `status.json` reads `state` `error`, `reason` `panic:<class>; restarting in <s> s` (class as in the incident catalog `stdio|unwrap|index|other`); the project log keeps the panic text; an incident report follows § 4.
  - Child handles live in a table owned by the project thread, outside the panicking state; the rebuilt state re-attaches them, so live children keep running and their leases stay listed in the heartbeat.
  - Backoff 1 s, 10 s, 60 s. Three panics in 10 minutes quarantine the project: a reduced loop only reaps, beats and writes `status.json`, reason `quarantined: panic:<class>`; no claim, reviewer or resume. `service restart` or an update clears it. If the reduced loop also panics, the thread stops; other hosts report its executions unknown and wait for attributable termination evidence (§ 3).
  - Other projects never pause: they share no lock with the panicking pass except host state (budget, admission, profile writer). Poisoned admission state holds launches until rebuilt from verified child handles; never grant from potentially inconsistent counters. Budget/profile errors report unknown without blocking other project loops.
- **Process fault.** An abort, stack overflow, OOM kill or a panic outside a project thread stops every project. The launcher restarts the unit and reconciles original native identities (§ 4 restart); unresolved executions retain their fence and occupied capacity. Neither restart nor a missing handle implies `gone` or release. This blast radius is the cost of option B; today it is one project.
- **Config.** An unreadable `taskq.json` holds that project only (reason in its `status.json`); a malformed `<install>/projects` keeps the last good list and reads unknown in `service status`.

## Defects this removes

| Class | Today | After |
|---|---|---|
| #1217 profile records lost | Up to six service processes plus CLIs append to `taskq.log` under a nonblocking lock; a collision drops the record and blames the log size. | One in-process writer for all service records; contention is service versus CLI only. #1217's bounded wait still applies to CLI writers. |
| #1084 board budget | Each service keeps its own request counts, its own rate-limit pause and its own hourly record; one service pausing on a 403 does not stop the others on the same token. | One counter and one pause per board host token; one hourly record per host; one update check per interval instead of one per project. |
| #933 duplicate service | A second unit for one root or host name (second Windows service, WSL plus native, a unit left after reinstall) serves the same tasks: false `gone`, two workers on one task. | One unit name per host, a host pid check and the per-root check; no per-project unit to leave behind. |
| Admission skew | `admission.json` across processes refuses grants while any service is older than the minimum version or its entry is stale. | In-process admission; migration holds new admission until legacy exit. |
| Mixed versions per host | Projects re-exec at different times; one host runs several versions at once. | One version per host. |
| Tray races | Any of N services may start the tray. | One spawner. |

## Implementation tasks

Each task is one reviewable diff. Lines `Depends on` go into the task body as written.

1. **coreq: host unit and rollback floor** (repository coreq). `service::host_unit(t)` for launchd, Task Scheduler and systemd; `service::legacy_unit(t, project)` for removal; launcher counts unhealthy starts of the host unit and honors a rollback floor. Tag a coreq release.
2. **Project threads and host policy in one process.** Migrate the six capacity copies and validate host.json once per pass; apply the § 3, § 4 and § 9 diff blocks. `service run` without a root reads `<install>/projects`, one thread per project with `catch_unwind`, child table outside the pass state, backoff and quarantine; shared budget, in-process admission, `<install>/service` pid check, `<install>/poke`. Chain checks: two fake projects, one panics every pass while the other admits and its child survives; concurrent project requests cannot exceed host limits; a project cap only lowers; with host runtime limit 2, admission.max 2 and project cap 2, one running attempt with its retained reservation occupies one slot, a second pending attempt fills the other, its transition to running keeps occupancy at two, and a third is refused until one exits; equal issue/attempt numbers in different projects remain distinct; conflicting migration copies leave every original unchanged.
   `Depends on taskqorg/coreq#<task 1>`
3. **Install, uninstall, stop, restart and status.** Apply the § 5 and § 8 step 5 diff blocks. Project list edits under `<install>/projects.lock`, host unit write and load, `uninstall --host`, host-wide `stop`/`restart`, per-project `status` blocks, host-level `capacity` from any folder and project caps, tray restart/stop labels. Keep permission/resource-grant authority unchanged (R9).
   `Depends on #<task 2>`
4. **Legacy mode and migration.** Apply the § 8 shared core and step 3 diff blocks. Legacy-mode detection, `handoff` reason, own unit removal with readback, host-wide admission hold until all legacy entries have verified exit, rollback floor. Chain check: legacy root with a live child admits nothing, keeps the child, then removes its unit and the host thread takes over.
   `Depends on #<task 3>`
5. **Release and host qualification.** Release N; on mac, Windows and FRA record per host: legacy units gone, every root served by the host pid, no `gone` from migration, rollback drills within the qualified execution-protocol floor, plus refusal below that floor, on macOS, Windows and Linux using isolated units and children; verify drain, exit, unit removal order, restored config and update hold before any live rollout.
   `Depends on #<task 4>`

## Open

- Host topology and drain-based handoff are accepted design decisions; implementation and installed qualification remain subject to H-A1–H-A5.
- The macOS `launchctl bootout` child-survival probe decides whether macOS may adopt instead of drain later; not needed for N.

## Contract detail

Host runtime detail for taskq.md sections 3, 4 and 9. Implementation and installed acceptance are tracked separately by H-A1–H-A5.

- Data ([specq data](https://github.com/taskqorg/specq/blob/main/engineering/data.md#files)): telemetry uses [SQLite/WAL bounds and migration](standalone-telemetry.md#sqlite-implementation-and-transition), pruned each host/project pass under the profile lock. Text-log bounds live in `src/data.rs`; legacy `taskq.log` is checked each host pass (each project pass on legacy services), under `taskq-profile.lock`, and keeps its newest half past the collection threshold of 4 MiB − 512 bytes. A busy lock or failed read/write leaves collection unknown and retries next pass. Other files are pruned hourly: `<project>.log` past 4 MiB and `.taskq/cli-errors.log` past 256 KiB keep their newest half; attempt logs the newest 50 plus live attempts; `.taskq/inputs/*.txt` the newest 50 plus files locked by live Claude turns; `incidents/` the newest 50; `events` follows [taskq.md section 4](../../taskq.md#4-service). A corrupt local state file reads unknown, admits or adopts nothing and deletes no release ([menu](menu.md#contract-detail)).

- Tray writer handover (R8, R12): after acquiring host ownership, a host with unavailable or incompatible telemetry IPC requests a fresh tray to exit through its heartbeat, then retries collection in a bounded window. No forced termination or store reset; unknown ownership keeps the error visible. A host still without a collector retries this capture each pass and logs one line per state change, so a tray heartbeat that turns fresh later hands over without a restart (#1336). The tray has no pass: its own thread retries a failed collector every 30 s and, after lost batches, logs one unknown line and one recovery line to `tray.log` (#1755).

- Release artifacts (taskq.md § 8 rule 1): checksums and provenance follow [security](https://github.com/taskqorg/specq/blob/main/engineering/security.md#dependencies-and-releases): each binary and `SHA256SUMS` carry a build provenance attestation (`gh attestation verify <file> --repo taskqorg/taskq`); installers and update check checksums.

- Host tools ([specq host requirements](https://github.com/taskqorg/specq/pull/98), #1856): the external tools taskq runs on a host, their minimum versions, checks and fixes are in [Host tools](#host-tools).

## Native stop and restart conditions

On Mac, Stop offers Drain (let owned work finish) and Full stop (cancel owned execution immediately, including offline) under R11/E2. Both persist the reviewed host hold before stopping; unknown execution or board reconciliation keeps completion unresolved. Other platforms retain Drain. Stop verifies the exact native daemon exit and retains the autostart unit. It does not add a login observer or timer. The Windows unit also repeats every 5 minutes (CoreQ, IgnoreNew), so an exited service starts again with no person while `service.lock` keeps a second one waiting; Stop disables only that repeat trigger, and the service enables it again (adding it to an older unit) each time it starts: logon, install or restart. The result states the native restart condition: macOS/Windows retain their logon trigger; Linux retains the enabled user unit, which starts with its user manager or an explicit service install. With linger, another SSH login does not restart that manager. Explicit service install can start the stopped host on every platform. On Mac, restart, logon and install do not clear the reviewed Stop hold: admission remains held until reviewed recovery (#1285). Ordinary `service release` refuses that hold; #1285 delivers its selective recovery, so this change provides no clearing path. Restart/stop/uninstall share command identity and observed native outcomes; none infer worker termination from the daemon's exit.

## Command-surface acceptance

Contract detail for R3/R8/R9 and sections3/4/5/8/9. These cases complete the accepted host transition; they are not evidence that it is implemented. Reuse the existing model/adapter tests and native qualification matrix.

| Case | Required observation |
|---|---|
| H-A1 Service entrypoints | Invocation from the install or an explicitly registered project selects the same host owner; no cwd-dependent second scheduler. Unregistered roots cannot register themselves through run. |
| H-A2 Policy surfaces | CLI, tray, operator and authorized remote resource edits use one canonical host-policy operation; actor/scope/grant/revision validation remains at each trust boundary. Equivalent accepted changes produce the same saved policy and effective service readback. Unsupported target/schema is refused before effects. |
| H-A3 Failure and restriction | Missing/corrupt/newer host policy holds new admission without fallback to project settings. Owned work remains supervised; restrictive commands retain their explicit receipt-failure policy. |
| H-A4 Shared occupancy | Two projects, pending claims, running children, reviewer/resume and unknown native execution cannot double-spend capacity; one pending-to-running transition adds no slot. |
| H-A5 Lifecycle and retirement | Install/restart/stop/uninstall/update preserve the declared scope and owned work; host unit/child absence is verified before retirement. Keep original configuration and qualified rollback; an incompatible old authority cannot restart against unresolved work. |

The existing <install>/service projection carries schema1, pid, native started identity, a random per-run instance, observation time and version (at most1024 bytes). Host startup retains the native lock before publication. Project status carries host_instance from that same in-process owner, including failed/reduced publications. Native identity determines whether that recorded daemon still exists; timestamp freshness is separate and cannot release ownership or workers. PID-only legacy records retain a conservative startup barrier but do not qualify the current host. Malformed/newer records stay unknown and are preserved. service status/list work from any directory; --json emits taskq.service/1 with service, projects and registry_error. Each project has host_binding current/other/unknown; absent or malformed generation stays unknown. Both renderers return host plus explicitly registered project observations; missing or stale cached counts remain unknown. This is observation, not install/stop/drain qualification.

Registry mutations share one editor holding projects.lock from preflight through commit. Busy edits refuse before native effects; existing unrelated path spelling and file ownership stay intact. Only initial registration may create a missing list. A retained projects.pending holds membership unknown and preserves the last supervised roots. Resource scopes are retired durably before removal commits, under the existing resource lock; if the registry write then fails, report the partial restrictive effect. Re-registration never resurrects the retired IDs. This data transaction does not qualify native drain/unit removal.

Installation uses the same bounded migration originals and command store. Before the first host activation, `<install>/host-policy-migration/install.json` binds the original launcher/current/previous, their usable releases, native definition, registry and first cohort's focus state; numbered byte copies live in `install-originals/`. The baseline is immutable and retained for qualified rollback, separate from the existing policy-migration plan. It is not execution/task authority. An interrupted unpublished snapshot can rebuild only its own fixed copy slots; unknown contents are preserved and refused.

A later cold installation captures the immediate pre-activation launcher/current/previous, saved native definition and GNU loader in one fixed `activation-originals/` copy set. Its metadata is carried by the existing installation cursor and bound to the original command receipt. With no committed cursor, recapture the present files under the same locks; never reuse an older caller's snapshot as current state. Active-host attachment does not activate or capture this set. Before any automatic restore, verify every copy and every active target against its original or the exact candidate activation result; changed inputs or ownership/permissions retain the cursor and refuse the whole restore. A native definition changed after the snapshot remains uncertain; a pre-submission error alone does not authorize overwriting it. The original migration baseline remains unchanged.

After restore preflight succeeds, retain the original failed command receipt before the first restoration write. A caller lost between files resumes that same failed command's restoration; it cannot reactivate the candidate or submit native startup. The cursor remains until restored readback completes; the failed command receipt alone does not prove that restoration finished.

`projects.installing` carries one schema1 installation cursor; `projects.pending` remains reserved for atomic registry publication. The cursor first commits an initial intent, then the original received receipt, before preparing active effects: original command identity, baseline/candidate digests, exact root, original/next registry and native phase. Lock order is update, projects, then resources. Both CLI/service update and the current-version fast path defer while the cursor remains. A replay validates the same binding and originals; it never issues another native start after a possibly accepted submission. A definite pre-submission failure may restore originals only with native absence and the owner lock; missing native state after submission alone cannot justify restoration. A verified saved/idle definition starts without rewriting it. On Windows a loaded task with no saved definition (an older installer's) gets `TaskQ.xml` exported from the live task; the export never replaces a file, reads omitted Scheduler defaults (`RunLevel`, trigger `Enabled`) as their defaults, and is removed again with one `schtasks` removal step when it fails the saved scope check (#1858). Windows registry entries store the plain `C:\` spelling; prefixed `\\?\` entries still match by canonical identity (#1858). Once a native host owns the transition, project/receipt uncertainty cannot roll topology back.

An existing live host advertises install_protocol1 before it can accept a new root. Attach changes only registry and that root's normal missing-focus default; it does not prepare/activate a candidate, rewrite/reload native configuration or change the floor. Compatible cursor readers use the protocol schema rather than equality with the installer's version. A host without that protocol refuses attachment before effects and needs the coordinated update/migration route.

After native ownership, a registered cursor exposes membership only when the registry matches its committed next value; admission stays held while project recovery/status proceeds. Project `host_install` binds acknowledgement to that installation ID and exact host generation, so an old status for the same PID/root cannot complete a new registration. CoreQ received/applied/observed receipts retain their original identity in the existing resources local_install binding; `service status --request ID` reads it after caller/cursor loss. These local bindings are pinned while unresolved and never enter remote resource exchange; terminal receipts keep the existing24-hour retention. Wait/failure reasons remain readable, and later verified progress clears the old reason. Implementation and real-OS/installed acceptance remain separate H-A5 gates.

Without a request ID, equal latest observation times report ambiguity and require `--request ID`; map order cannot select another command's result. A corrupt retained installation receipt keeps its installation identity and reports the missing proof as unknown.

The existing single background maintenance owner reconciles a retained installation instead of checking for another update. It carries the original command context and ID; an absent or different cursor cannot create a new command. Exact candidate version and installation protocol are checked before publishing membership under another host generation. The caller releases the update lock after the applied registry commit; final acknowledgement takes update/projects locks without waiting. Competing observers read the same retained observed result after cursor removal. This recovery does not start a second permanent helper or block the host heartbeat loop.

The first topology originals include the exact pre-host rollback-floor bytes/metadata or its recorded absence. The launcher floor applies to legacy service starts too; keeping N active while selecting N-1 prevents the reverse procedure from starting its units. Owner-only reverse therefore needs the original topology selection, including its qualified prior floor, after verified host removal; a missing original floor record is unknown, never permission to delete a floor. Pre-native activation recovery does not change the floor. Preserve an independently readable copy of host policy and originals outside older-version cleanup before invoking that older installer or releasing its update hold.

Native legacy entry routing reads the daemon's actual manager binding, action, working directory, creation identity and saved-definition revision. The original native label remains authoritative after a directory rename. Windows also binds the launcher creation identity and Scheduler instance. A failed scope read holds startup instead of falling back to host mode. A verified legacy owner uses its original project policy, enables handoff before adoption/board processing, retains handles across partial adoption or loop failure, and admits no fresh worker/reviewer. It does not consume update/restart signals or start operators/trays while draining; existing operator state remains a wait. Its status identifies the native scope, handoff error and ownership waits. Native retirement, full transaction handoff and installed acceptance remain required; identifying a legacy owner does not authorize removing its unit.

Drain/re-exec preparation uses the service's reconciled ownership, not child count alone. A failed or absent ownership read, unresolved board execution without a retained native handle, held claim or pending release prevents completion; an uncertain reservation prevents drain, not the [re-exec](#restart-handover). A project with invalid configuration cannot certify idle. Reduced monitoring retains the last complete ownership read and all unresolved entries; after a failed new read it cannot certify idle. Unix re-exec may retain its qualified CLI handles under section8; removing a project or stopping its daemon requires no retained execution or active operator. Restart never drops unknown slots merely because their project thread ended. During Stop, a prepared project with no busy work or native slot stays latched; wakeups cannot revoke its preparation while other projects drain. Active or unresolved projects continue monitoring.

| Ownership trace | Drain | Re-exec |
|---|---|---|
| No successful pass, including configuration failure | Wait: ownership unknown | Wait |
| Unresolved local execution, no child handle | Retain project and slot | Wait |
| Terminal evidence not durably released | Retain project and slot | Wait |
| Successful reconciliation, no owned work or operator | Ready | Ready |
| Retained qualified CLI child | Wait | Unix handover only; Windows waits |
| Childless reserved or refused attempt (#1601) | Wait; Stop: ready, the next start resumes it (#1769) | Ready: the next image resumes it from the board |
| Held launch (#1601) | Wait | Released `no-start` first, then ready |

Each project's preparation is the report of its last finished pass; a pass in flight holds the re-exec. Once every project and retained slot has a prepared owner, preparation latches: no project starts another pass, inventory refreshes stop, and the host re-execs when the passes in flight report prepared, so overlapping project passes cannot defer the handover (#1402). Failed threads lose preparation, retaining unknown occupancy. This closes the check-to-re-exec gap without a new persistent record. A pending handover wakes each project once; later passes keep `poll`, and a failed legacy handover retries at `poll`, so a waiting handover lists the board at most once per project per `poll` and completes within one `poll` after live work ends (#1357).

### Restart handover

A pending restart (re-exec or Stop) refuses every reservation (`Host::reserve`: `host restart pending`). The re-exec therefore waits only for what resolves without one: live children under § 8 step 3, ownership reads, board executions without a child (Unix) and unconfirmed releases. A childless reserved or pending attempt, such as an answered task whose resume the restart refused, passes to the next image, which reads it from the board and resumes it from the inbox (#1601). A held launch never called its start: the pass releases it `no-start` and the next image acts on the board; an initial claim released this way ends `gone` with one retry spent.

Stop applies the same rule (#1769): it drains running executions only. A released attempt whose resume the Stop refused stays owned on the board and resumes after the next start. A Stop that waits past its expiry names the waiting tasks in its receipt reason, so `service status` shows what blocks it.

The next host generation drops a Stop issued by an ended generation: at once while it is still queued, and after its expiry once applied. Only the current native owner (fresh `<install>/service` with its own instance and pid) may drop it; the reviewed Stop hold stays until reviewed recovery. Without this, a Stop of a dead generation held admission with no command to clear it.

Why: the old rule waited on those attempts while the restart refused their reservation, so neither side could move and the host stayed on the old image until an owner kickstart. Admitting slots under a pending restart was rejected: a child started then must itself hand over or delay the re-exec again. A test sets up each childless attempt kind and asserts one pass reaches the re-exec.

While the re-exec waits, the host retains the projects' `restart_waits` as the reason of a queued upgrade receipt (`host re-exec into <v> waits for <project> <wait>`), and status shows the same list. A re-exec still pending after 600 s logs `restart pending over 600 s; waits for …` once and reports `dispatch.reexec` (`restart:stuck`); nothing is killed or restarted.

A newer `service install` build supersedes a queued upgrade intent for an older version: the old receipt fails with `superseded by <v>`, the intent goes, and the newer build requests its own upgrade. A build equal to the intent still continues it.

Known unresolved executions stay in the existing ownership projection when a task closes or loses its task label. Read their trusted board history directly when absent from the open-task views; only the matching terminal release retires childless ownership. Failed or missing history remains unknown. This read does not submit, cancel or recreate work; native handles still require their own terminal observation.

Tracked host Restart uses the existing resource-state/state.json operation bindings and CoreQ command receipts. Its immutable local request names the reviewed canonical registry and original service instance; only a fresh native-bound owner advertising restart_protocol1 accepts it. CLI works outside project directories; the tray reviews every affected project. A worker cannot submit this host operation. Submission expires after at most300s; an accepted pending operation stays pinned within the existing4096-binding bound, and terminal receipts retain the existing24-hour window. Remote exchange/cancellation cannot access local bindings. Registry edits wait while Restart is pending. A request is received before persistence, applied before the qualified image switch, and observed only after another service instance publishes successful recovery for every reviewed project. A crash or lost response reconciles the same request instead of requesting another switch. Failed receipt storage, changed scope, corruption or unsupported data stays unknown. These are source/installed qualification targets; legacy flags stay identified compatibility inputs until retirement.

Stop shares that lifecycle reader/writer. Existing local_restart records remain valid; local_stop binds the same immutable scope/instance with action=stop. A fresh stop_protocol1 owner and a readable retained host unit are required before submission. Preparation requires no owned execution, slot, operator or update; eligible Unix CLI handover alone is insufficient. The original applied receipt binds native pid/creation identity and the retained unit's SHA-256 before dispatch. A short controller runs outside the stopped unit: macOS process_group0, a Linux transient user unit, or a Windows interactive task with a120s lifetime and no trigger. No per-execution launcher, permanent service or new store is introduced. CLI status/list adds lifecycle for either command; its restart field remains a compatibility projection for Restart only. Tray uses one lifecycle controller, with a separate immutable review for each action.

The controller validates unchanged scope/definition and native unit ownership, performs native Stop, verifies exact daemon exit and exclusion of a new host owner, then commits the original observed receipt. Before the Windows native effect, the existing Stop receipt also binds the scheduler instance GUID and launcher pid/creation identity. Replays retain that scope after a lost response; an unattributable live daemon remains unknown. Windows stops the matching scheduler launcher before kernel-identity-bound termination of its idle daemon; PID/image-name tree killing is not used. Its temporary controller task explicitly permits the current user and SYSTEM to manage that task, so a limited controller can retire its own definition even when an elevated caller registered it; host/worker grants are unchanged. The host unit stays installed. The result remains queryable through service status --request after UI/service exit. Same-ID replay returns retained evidence; required storage/native failure remains applied/unknown with a bounded reason. A new host generation cannot falsely acknowledge the old Stop. Native controller/task-scope and installed-candidate acceptance remain distinct qualification gates.

Uninstall uses that same retained lifecycle protocol, advertised by uninstall_protocol1. A project request names its exact canonical root and reviewed registry; the root remains registered while its thread drains, so a host restart cannot forget its work. Other projects keep running. Only a qualified drained thread may apply registry removal and resource-scope retirement; observed means readback of that removal. Removing the last project additionally stops the exact idle host, removes only its native unit and verifies absence before the original receipt completes. uninstall --host drains every project and removes the native unit while preserving the project registry and configuration for rollback. Both forms retain original receipt identity, reject workers, survive caller/UI loss and reconcile the same ID; native/storage uncertainty never becomes success. Existing Restart/Stop bindings stay compatible.

Explicit Windows host installation gives the current-user SID and SYSTEM management rights to only the owned TaskQ host task, using CoreQ's host installer. Its daemon/controller remain LeastPrivilege; worker/project grants stay unchanged. Existing task ACLs are not repaired by runtime or Uninstall. A default-ACL task created by an elevated installer may permit Stop but deny removal; report that native refusal and require explicit installation repair. Qualify the installer/consumer revision before installed acceptance.

Cold host startup reuses the existing slot-rebuild barrier: no project admits until every registered project has accounted for its prior occupancy. Missing configuration or board ownership keeps the barrier unknown, not empty; published host use is null during that recovery. Restart observation additionally requires every unresolved execution to have recovered its retained native ownership, separately from successful task snapshot publication.

The host owns one bounded update thread. A running check defers image replacement while project heartbeat/native monitoring continues; it does not block host publication. Host/UI control observers use the same resource lock nonblockingly and report its busy state. Required receipt failures persist a bounded reason in the existing operation receipt; a later successful observation clears it. A busy lock or failed update never proves that owned work stopped.

Local capacity writes reuse CoreQ command receipts. The writer submits received before the effect, then replaces policy and its `_taskq_capacity_command` descriptor together in host.json: schema1, host, normalized capacity snapshot and encoded receipt. A missing collector refuses before mutation. Receipt commits never block the save (#1351): a slow or budget-locked store leaves the phase unknown in the host log, the policy is saved and the effect is never repeated. The host submits applied/observed only for the exact loaded descriptor whose host and capacity still match the current file, under the existing resource lock. It can recover a writer death after replacement using that same descriptor. Equal values or a fresh status timestamp cannot acknowledge a different command. A manual capacity edit invalidates the previous command observation; malformed/newer descriptor data is preserved and holds policy unknown. No second command file, journal or database is introduced. Existing telemetry retention bounds the receipt history; absent history remains unknown.

Native tray requests carry id, issued_at, expires_at and typed host-capacity ops, each with the reviewed revision. The ID binds the full request in existing resource-state/state.json, sharing its4096-binding and24-hour-after-expiry limits. Lifetime is at most300s and the local clock permits at most30s ahead; check again immediately before replacement. A durable writing marker precedes the effect. The host.json descriptor adds the request ID for read-only recovery after a lost result; replay returns retained evidence, never repeats an uncertain write. Without an attributable binding the outcome stays unknown. Validate registration, local installation/generation, host/runtime target, schema and field types; legacy project payloads cannot edit host policy. Local receipts never enter the DashQ exchange and remote messages cannot cancel/reuse their IDs. Tray edits use a bounded worker independent of the restrictive focus queue.

Update cleanup preserves the host policy and its pending replacement, registry locks/prepares, native recovery directory, migration originals, host control signals and rollback floor. An unreadable registry or a registered root without a readable version makes release ownership unknown; preserve releases until readback resolves it. Cleanup never supplies missing termination or retirement evidence.

The transition owner is the host service. Project loops and status are consumers/projections, not fallback authorities. Legacy project units, capacity_hosts/mode and admission.json expiry may remain only for identified migration/compatibility readers; after cutover no supported writer/admission path may use them. Retire them only after H-A1–H-A5 and the existing installed acceptance pass. Exact receipts and native release evidence remain required.

## Shared core mapping

Contract detail for taskq.md section8.

**Shared core.** Release, update, launcher and rollback, service units, the `status.json` common keys and the GitLab board token follow [coreq.md](https://github.com/taskqorg/coreq/blob/07624c840bdaeae57caa6409c6035f7479a20138/coreq.md) (crate `coreq`, immutable revision in Cargo.toml/Cargo.lock; JS packaging uses the same revision) as `Tool::new("taskq", "TaskQ", "taskqorg/taskq", VERSION)`: install `~/.local/share/taskq` (Windows `%LOCALAPPDATA%\taskq`), files `taskq-<ver>-<os>-<arch>`, host units `com.taskq.service`, `TaskQ`, `taskq.service`; per-project units are legacy migration inputs only.

### Receipt-degraded Stop (R8)

The § 4 exception applies only to Stop on an upgraded service advertising protocol 2. Exhausted legacy protocol-1 Stop is refused before intent is written; automatic recovery covers the budget mismatch, not an unavailable or exhausted legacy store. A missing collector may supply an identity descriptor without claiming a telemetry commit. Received/applied/observed commits run beside the lifecycle step and never block it (#1351); a commit that does not land is unknown in the host log. The `telemetry_unknown` record field stays readable for older records. This never substitutes for native ownership, project drain, registry locks, unit revision or attributed exit. Restart and Uninstall retain mandatory telemetry receipts; a Restart whose CLI has no collector instead writes the untracked `<install>/restart` file and reports `untracked`, `telemetry: unknown`. Old readers refuse the extended degraded record, so the service must be upgraded before it can complete that path; no old process is forcibly retired.

## Default runtime

Detail for taskq.md sections 4 and 9 (#1668). Owner decision 2026-10-23 ([specq#89](https://github.com/taskqorg/specq/pull/89)): FRA runs without claude for now; work and review there run on codex. A host the owner runs without claude therefore sets host.json `default_runtime` to `codex`; hosts with claude keep the current rule.

- Values: `claude` or `codex`; an absent key reads `claude`. Any other value makes host policy invalid, which reads unknown and holds launches (section 9).
- Route: a reviewer always uses the host's default runtime. A worker uses its `run-*` label; without one, a lone legacy `prefer-codex` still routes to `codex`, and otherwise the worker uses the default runtime. The task waits for a free slot of that runtime and never moves to another.
- Review: a codex reviewer gets the same review rules, `.worktrees/review-<N>` worktree, access grant check and verdict path (close, requeue, ask) as a claude reviewer; nothing in the review flow branches on the runtime name.
- Command: `taskq service capacity --host H --default-runtime claude|codex` writes the key through the same atomic host policy operation as the other capacity edits and prints `Default runtime: <rt>`; an unknown runtime is refused with the valid names. A read prints the same line, and `taskq service status` shows it per project as `Default runtime: <rt> (host <name>)`.

## Dev install

Detail for taskq.md section 8 item 1 (#1388). Applies only on macOS, only with host.json `channel` `dev`, only in the project thread whose `repo` is `taskqorg/taskq` (#1406). The root's working tree needs no `scripts/dev-install.sh` and stays untouched: a root checked out before the script existed installs too. Channel `live`, Windows and Linux hosts do nothing new; a `dev` host still logs `channel dev: release <v> not applied` once per skipped release.

- Trigger: each project pass reads the `origin` `main` SHA with `git ls-remote` (no board request). A SHA that is neither the installed one nor already started reads `pending`; its `merge_group` CI is read through the board at most every 5 minutes, with the same check that close uses (§ 7). Red, pending or missing CI waits.
- Run: on green CI the service writes the SHA to `<install>/dev/started`, logs `dev install: start <sha>` and starts, in its own process group, the `scripts/dev-install.sh` of that SHA (taken from the fetched commit; the fetch leaves `origin/main` untouched, so a concurrent fetch in the root cannot fail it, #1633) with output in `<install>/dev/install.log`. The script holds `<install>/dev/lock` (its pid), so one install runs at a time, manual runs included; a newer merge during a build is picked up by the next pass after the lock frees.
- Handover: the script installs exactly as a manual run does (§ 8 steps 3 and 5, #894, #1306); it changes no `update_every`, load gate or worker.
- Result: the script writes `<install>/dev/result` (`<sha> installed|failed`) on exit. The next pass logs `dev install: <sha> installed|failed` once and moves the SHA to `installed` or `failed`. A failure before `service install` keeps `current`. A started SHA is never started again, so a failed, killed or unreported build waits for the next merge or a manual `scripts/dev-install.sh`.
- Display: `status.json` key `dev` holds `installed`, `pending`, `failed` and `building`; `taskq service status` prints them on its `Dev:` line.

## Host tools

Contract detail for taskq.md sections 4, 5 and 8 (#1856, owner decision 2026-10-29, [specq#98](https://github.com/taskqorg/specq/pull/98)). Each external tool taskq or its workers run on a host, its minimum version and the owner's fix. `src/tools.rs` holds the same table, its runtime rows in the runtime adapters; a new tool, flag or API use raises the minimum in the same change, and its tests use a fake of the oldest or a missing version.

| Tool | Minimum | Needed for | Required when | Fix |
|---|---|---|---|---|
| curl | 7.55.0 (Windows 10 1803 `curl.exe`) | update, board API (`-K -` config), rules download | always | install or update curl |
| git | 2.17.0 (`worktree remove`) | worktrees, tidy, recovery, worktree seed, private fallback, dev install | a project is registered or found | install or update Git |
| gh | 2.5.1 (`gh pr checks --watch --interval`) | GitHub workers: `gh pr create`, CI watch (§ 7) | a GitHub project is registered or found | install or update GitHub CLI |
| claude | 2.1.296 (oldest [qualified](../runtime-qualification.md)) | claude runtime | host.json `default_runtime` is `claude` (absent key: claude) | `claude update` |
| codex | 0.159.3 (oldest qualified) | codex runtime | host.json `default_runtime` is `codex` | update Codex CLI |

- Check: `taskq selftest` runs `<tool> --version` for each row and prints `tool <name>: <found> (minimum <min>) PASS`. A required tool that is missing, below its minimum or whose version is unreadable prints `tool FAIL <name>: <found>; minimum <min> for <need>; fix: <step>` and fails selftest. An installed optional tool below its minimum fails too; a missing one prints `not installed (optional)`. gh is not listed on a host with only GitLab projects. A host without host.json (release runner) requires no runtime and a host without a project requires no git, as § 8 release hosts need none.
- Offline: the staged release selftest (`TASKQ_SELFTEST_OFFLINE`) prints `tools: unknown` and probes nothing, so a host gap never holds the update that may fix it. The dev install build check and the tray's Add project check set `TASKQ_SELFTEST_TOOLS=off` the same way: a tool gap neither fails a dev install nor classifies a taskq project as old; `service install` then prints the gap.
- Install, onboard and update: `service install`, `service onboard` and `taskq update` print each gap as `Tool: <name>: <found>; minimum ..` and `Next: <fix>; taskq selftest fails until then`; a service start (install, update, logon) logs the same lines. taskq never installs or updates a third-party tool (§ 8 Folders): the owner runs the step, and selftest stays failed until then.
- Failed calls: a tool call that cannot start, times out or exits nonzero is logged or returned with its command and error (exit status and stderr), never read as an empty result; an expected nonzero answer, such as a signature that does not match, is the result itself. `tools::run` renders the command; a call whose arguments carry a brief names only the program.
- OS components: launchctl, plutil, codesign, security, open, osascript, sandbox-exec (macOS); systemctl, systemd-run, the bwrap helper (Linux); PowerShell, tasklist, reg, WebView2 (Windows); and hostname ship with each supported OS and have no separate minimum. The Linux release needs the glibc of its build runner; the loader refuses an older one before selftest runs.
