# #2184 native A6 qualification (PM run, Mac)

Verdict: **PASS** for the seven additive stages (run 2). Run 1 hit an open gate (host update handover, #2209); the owner-side restart at 16:46Z cleared it.

## Candidate

- Source: `origin/main` 0778be2fdf725f96a2aa886d4803f556e5b4b08d, contains 2b8f14d8ae7c8986c8fb9021959ea0a37da3fbe1.
- Isolated detached worktree outside the repo; `npm ci --ignore-scripts`, then `cargo build --locked --bin taskq` and `cargo run --locked --bin taskq -- --version` (signing runner, `scripts/macos-runner.sh`). Prints `4.6.59`.
- Staged artifact `target/debug/taskq` sha256 `a4a5b3f9e63680755dec2dfcc73357e281327af86abec4cf6068f0e36b5b3457`, `codesign` identifier `org.taskq.taskq`. Not installed.
- Resolved native Codex: `codex-cli 0.159.3`, `codex.js` sha256 `61b0194f3bb6534439c8d26a3ed57d0805f84b884588b761795323eeb92fcf70`.
- Consumer driver: [qualify.py](qualify.py) (sha256 `4f2d8d4bdfdf20b436761711d0b5e09fd3de674685ab635625db54a24b79c861`). It is the real consumer process; its PID binds the slot. The release key stays private; the log does not contain it.

## Authority readback (before the run)

- `taskq service permissions` in `/Users/kirs/Projects/assetq`: `codex: grant workspace (project) rev 34 by kirs · effective workspace`.
- `taskq service capacity`: Codex 2/10, host 3/10 held, load gate Ready.
- Installed service: 4.6.60-dev.1793631711 runs; `current` is 4.6.60-dev.1793636541; `projects.upgrading` present; `dev/install.log`: "host still runs 4.6.60-dev.1793631711 (host update step pending ...)".

## Run 1 (2026-11-02 16:40Z, [run1-gate.jsonl](run1-gate.jsonl))

```
python3 -u qualify.py <candidate> /Users/kirs/Projects/assetq <log>
```

1. `service slots --identity <consumer pid> --json`: pid 83848, boot 1793499830.
2. `service slots --action reserve`: `state: unknown`, `stored_state: queued`, "fresh matching host readback required".
3. `observe` every 5 s for 290 s: never `reserved`; final `state: denied`, `reason: denied: admission deadline expired`, `occupied: false`, host instance `b9e47d22-a637-4b8c-a11c-d4b4e9fff915`.
4. The driver stops at "no slot". No `prepare`, launch, handshake, interruption, reconcile or release ran. A later readback still shows `denied`, `occupied: false`.

## Run 1 open gate (resolved)

Host slot admission is held: the Mac service is in an unfinished update handover (restart pending; `src/service.rs` holds every launch while "host draining, restart or hold"). This is taskq#2209. The PM brief forbids touching the service install, so admission cannot be obtained now.

No grant, `current`, capacity, account or native configuration change. No model turn, paid call or forced release. The denied slot holds no occupancy.

## Run 2 (2026-11-02 16:48Z, [run2.jsonl](run2.jsonl))

Service restarted by PM at 16:46Z, now 4.6.60-dev.1793636541, host instance `2985e87c-3679-4185-89b6-ea95b4f99f7a`. Same candidate (sha256 `a4a5b3f9…3b457`), same driver.

| Stage | Readback |
|---|---|
| Slot | reserved in 18 s (`granted_at` 1793638146) for consumer pid 8938 |
| Caller-bound prepare | `prepared`, revision 34, account `default`; repeat identical; wrong key: "slot belongs to another caller, payload or release key" |
| Drift refusal | changed digest: "independent preparation missing, stale or changed; no native dispatch/input"; revision+1: "project Workspace authority missing, stale or mismatched"; observe after both: "launch identity absent", so no launch was consumed |
| Scope-control query and boundary | observe `verified-effective`: live native image plus named sandbox queries; workspace write allowed; policy, launcher, native executable, account config and home write/create/unlink denied; scope-control read/write/create/unlink denied (the receipt fails otherwise) |
| Exact stdio peers | `dispatch_ack: true`; frontend 9918, supervisor 9932, native 9955; supervisor coalition 2959 differs from the frontend's; `initialize` answered by codex 0.159.3, then `initialized`; no turn |
| Inherited lifetime through interruption | frontend SIGKILL; release refused "protected descendant completion unproved; retain original slot"; relaunch refused "slot launch already consumed; observe the same operation, never replay" |
| Sealed singleton certificate | `taskq.native-completion/1`, coalition 2959, counts [39, 38] (only the supervisor left), `input_closed: true` |
| Cleanup acknowledgment recovery | `reconcile`: `cleanup_intent`, `bootout_ack`, `cleanup_verified` all true, `completed`; duplicate reconcile returns the same completed receipt |
| Original-slot release | release accepted (`releasing`), readback `released`, `occupied: false`, "consumer confirmed entire turn ended" |

After the run `pgrep` finds no `scope-run` or `codex app-server` process.

Limits: interruption covered the frontend only (root and caller ended normally after it). Delegated remote/XPC/tool effects are out of scope, as the design says. Handshake and boundary evidence that did not change was reused, not run again.
