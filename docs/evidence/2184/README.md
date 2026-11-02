# #2184 native A6 qualification (PM run, Mac)

Verdict: **open gate**. A6 stays unverified. No native stage ran.

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

## Run (2026-11-02, [qualification.jsonl](qualification.jsonl))

```
python3 -u qualify.py <candidate> /Users/kirs/Projects/assetq <log>
```

1. `service slots --identity <consumer pid> --json`: pid 83848, boot 1793499830.
2. `service slots --action reserve`: `state: unknown`, `stored_state: queued`, "fresh matching host readback required".
3. `observe` every 5 s for 290 s: never `reserved`; final `state: denied`, `reason: denied: admission deadline expired`, `occupied: false`, host instance `b9e47d22-a637-4b8c-a11c-d4b4e9fff915`.
4. The driver stops at "no slot". No `prepare`, launch, handshake, interruption, reconcile or release ran. A later readback still shows `denied`, `occupied: false`.

## Open gate

Host slot admission is held: the Mac service is in an unfinished update handover (restart pending; `src/service.rs` holds every launch while "host draining, restart or hold"). This is taskq#2209. The PM brief forbids touching the service install, so admission cannot be obtained now.

Stages not qualified: caller-bound prepare and drift refusal; scope-control query; exact stdio peers; inherited lifetime through interruption; sealed singleton certification; cleanup acknowledgment recovery; original-slot release and readback.

Rerun after #2209 lands and the Mac service runs the current build: the same driver, the same candidate commit or a later main that contains 2b8f14d8.

No grant, `current`, capacity, account or native configuration change. No model turn, paid call or forced release. The denied slot holds no occupancy.
