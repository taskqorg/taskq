# Check results

Attempt `6537039713`; application source/binary identity is in [manifest](manifest.json). No product code, tests, guarding assertions or rules changed. The new JSON/PNG evidence makes the existing selector broaden to `full`; no selector/gate was weakened (R12/R13).

| Command / observation | Outcome |
|---|---|
| `cargo run --locked -- tray --open` with own install/TMPDIR/current | Pass for isolated native application startup; the original no-current exits are explained in [checkpoint](README.md). |
| `codesign --verify --strict` on isolated app | Exit 0; separate qualification identifier and existing signing identity. |
| CUA native application sequence | Actual draft/file/detach/archive/client-restart subsets pass; exact limits and tool targeting correction are recorded in [checkpoint](README.md). Whole A2/A4 remain Not tested. |
| Native store readback assertions | Pass: text/files/archived/caret equal before/after client restart; one cancelled request, null job/session; no service owner record. [Readback](readback.json). |
| Isolated `cargo run --quiet --locked -- service run` | Exit 1, `a worker cannot run the host service`; expected authority refusal, not an implementation defect or native provider pass. |
| `service status --json` | Unknown, not healthy: host service owner record missing. [Observation](service-status.json). |
| `git diff --check` | Pass. |
| Changed report/checkpoint local links and evidence JSON parsing | Pass. |
| `python3 scripts/check.py --base d257507aed50788e831af01c1d4f7fafcb468399` | **sandbox-blocked, not PASS**; selected full; exit 1; 96.178 s / 900 s; no retry. |

The selected gate passed its own selection/authority/rule-gate checks, npm installation/build and 752 Rust unit cases (35 ignored). The Rust integration result was 21 passed, one blocked, one ignored. Exact failing case:

```text
telemetry_process::resume_records_its_result_without_releasing_the_host_hold
tests/telemetry_process.rs:189:9
sandbox-exec: sandbox_apply: Operation not permitted
Selected gate elapsed: 96.178s / 900s; no retries
```

The same host-sandbox limitation was already recorded in the earlier checkpoint. No sandbox setting was changed or retried. Later canonical stages were not reached and are not marked passed. No CI poll, merge, rollout or production install occurred; merge-result CI remains pending. §7.3 parks on the separate owner-run context blocker #2003, not on this sandbox-blocked check.
