# Legacy handoff and rollback verification

Task #1232 verifies the integrated #1231 handoff and repairs admission progress after a native inventory refresh. R12 and §§3,4,7,8 separate source, process and native evidence. No rule or acceptance protection changes.

## Exact source handoff to #1233

- Integrated TaskQ baseline: `7006084c9ed68db8734e8587ee936b9c5d2e82d0`, the reviewed #1231 / PR #1261 merge. Its result source was `69054e9b80414747babe015041fc12de2070a57c`.
- CoreQ: `5fb10926bbca0fdb042342dc5dc6ac3cc4e5a56f` (0.14.4), unchanged in Cargo.toml/Cargo.lock. The prepared TaskQ source and accepted Mac evidence remain linked in [#1231 handoff](qualification-1231.md).
- This candidate changes only host inventory freshness/wakeup, its regressions and this report. The final #1232 result and PR head identify its immutable commit. Tested source blobs: `src/host.rs` = `56908c5bcc63aef944452bfae0b5827663532d1d`; `src/service.rs` = `cd8e1da85e8a733d78ac5225b7f32a5484ccc438`.
- `src/host_install.rs`, `src/host_control.rs` and `src/legacy.rs` are byte-identical to prepared source `506a1ae14fc02f1d2b722d6a973fbd504b2ee3aa`. The integrated publication changes remain in service.rs; they are not replaced by the prepared version.

## Defect and repair

A successful zero-unit native inventory did not wake project loops sleeping after a blocked pass. Their default 60-second poll could meet the inventory's 60-second expiry again. Freshness was also measured from dispatch rather than completed observation. This explains a possible starvation sequence; it is not proof of the historical production failure's complete cause.

Before the repair, `cargo +stable test --locked fresh_native_inventory_wakes_pending_work -- --nocapture` failed: `fresh zero-unit inventory must wake pending review/start`; both poke observations remained absent. The setup's first build required the locked Node dependencies; that build failure was not counted as a behavioral reproduction.

After the repair, the same regression passes. A completed native observation starts its freshness interval. A transition from blocked to usable host admission pokes registered projects after publishing the new policy/barrier in memory. No focus rewrite, service restart, weaker stale barrier or new grant is required. Failed wakes are logged; a wake itself never grants capacity or bypasses any admission condition.

The `native_inventory` test selection passes all three checks: expiry at 60 seconds and unknown/retained-unit cases still refuse admission; existing reservations survive; fresh inventory wakes the actual sleep helper; the service pass then starts either a pending worker or pending reviewer on a disposable board. The focus file stays byte-identical. These are component/process-adapter checks with fake boards/children, not a paid native agent turn.

## Preserved handoff and rollback protection

| Acceptance | Executable evidence and boundary |
|---|---|
| A1 | The integrated contract §3/§8 and host-service migration/rollback detail retain legacy ownership, no new legacy claims, drain, verified exit, unit removal, host acknowledgement and floor recording. The three retirement/installation adapters above are unchanged. |
| A2 | `legacy_handoff_preserves_active_work_before_native_retirement` passes the existing isolated installer/project-loop process chain: live child and legacy PID remain through host start; preparation follows child exit; the manager substitute acknowledges actual legacy process exit before unit deletion; the same installation receipt observes host takeover. The native manager and board are substitutes. |
| A3 | `legacy_handoff_preserves_owned_answers_and_blocks_new_claims`, `prepared_legacy_drain_defers_late_answers_without_replaying_work`, `legacy_cursor_requires_matching_drain_and_exit_proof`, `pid_check_and_poke_use_host_paths_and_unknown_is_held` and the inventory checks pass unchanged negative protections. Unknown ownership cannot synthesize exit or release admission. |
| A3 floor | On exact CoreQ source above, `cargo +stable test --locked launcher::tests:: -- --nocapture` passes 6 tests, including `floor_recording_and_host_counter` and `host_rollback_respects_floor`: an unhealthy host cannot select N-1 below the floor, a below-floor current selection is refused, malformed floors fail closed and lowering the established floor is refused. This verifies the launcher selection boundary, not a manual invocation bypassing that launcher. |
| A4 | This report plus the #1232 immutable result/PR is the source handoff. #1233 owns release/activation; no production service was stopped, restarted, installed or otherwise activated here. |
| A5 | The before/after regression and pending start/review chain above retain the 60-second expiry, refresh and wake sequence without rewriting owner focus. |

## Candidate gate and limits

With worker and trace variables removed only from the test process, `python3 scripts/check.py --base origin/main` selected the full gate. It completed 377 unit and 16 integration checks; 30 unit and one integration instruments remained ignored. The only executed test failure was `resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. The gate exited nonzero after 23.314 seconds: **sandbox-blocked, not PASS**. No assertions were weakened, tests skipped to manufacture success, permissions changed or passing suite repeated.

An isolated real launchd probe attempted only its unique `com.taskq-legacy-probe.fixture...` label and a temporary HOME/definition. `launchctl bootstrap` returned exit 5 before an owner record appeared; readback found that exact label absent. Native inventory was readable, but bootstrap's underlying failure cause is unknown. This attempt does not qualify native retirement. Process inventory via `ps` was also denied by the worker sandbox. No production label was submitted or removed.

The prior accepted six-project Mac topology evidence is reused only for its recorded source/environment. Fresh native unit retirement and an installed eligible queue progression remain unproved here. Exact-result-SHA CI remains the §7.4 acceptance gate; #1233 must retain the native qualification limitation before activation. No Windows, other host, working profile, model, effort or runtime grant changed; no paid first qualification turn was replayed.

The remaining canonical stages were run once after the blocked Rust stage, without rerunning passing tests: offline `cargo +stable run --locked -- selftest` passed 82 golden traces, formats and committed local capture; its board read is explicitly unknown. All Node component tests, shots-gate fixtures, release-tag fixtures and exact-source release checks passed. These outcomes do not turn the blocked full gate into PASS. `git diff --check` passed. The result receipt preserves this distinction.
