# Owner-weighted balancing: acceptance evidence

[Task #2189](https://github.com/taskqorg/taskq/issues/2189) supplies the accepted goal and A1–A7. The implementation changes §4 and adds §4 Balancing before changing admission (R13). No installed policy, account, grant, active attempt or live service was changed (R9).

## Before and after

`cargo test --locked --bin taskq weighted_balancing_service_claim` failed before service integration: weights Claude=1 / Codex=3 still launched Claude instead of Codex. The unchanged expected runtime passes after integration. The first build needed the existing `npm ci --ignore-scripts --no-audit --no-fund` prerequisite; no dependency was added.

The two-host chain initially exposed a missing tie cursor after a successful launch. Recording the accepted spawn before the native launch fixes it; the claim fence still decides ownership. Re-reading the published heartbeat through a restarted service retains the cursor.

The legacy balancing filter initially ran with the launching worker's identity and rejected an owner-only resource fixture. It passes under the existing `scripts/check.py` test environment, which removes that identity for fixture children. Product authorization and the original assertions are unchanged.

## Acceptance mapping

| Acceptance | Executable evidence |
|---|---|
| A1 | `taskq.md` §4 Balancing, configuration/capacity/command details and the Board cutover note; original owner request linked above. |
| A2 | `host_policy::weighted_balance_cli_preserves_host_policy`: actual CLI save/read/replace/clear; malformed, duplicate and worker writes refuse without effects; limits, permissions and unknown owner fields survive. |
| A3 | `balance::tests::weighted_runtime_selection` and `service::tests::weighted_runtime_hostwide_quota_selection`: workers/reviewers, host-wide occupancy across projects, native quota observations and low-headroom ranking. |
| A4 | Weighted selection and host-wide quota tests retain holds and strict pins; no weights keeps existing preference/default fallback. Existing disabled-pin auto_reroute policy is unchanged. |
| A5 | `balance::tests::weighted_host_spread` and `service::tests::weighted_balancing_host_claims_and_status`: free weighted capacity, tie rotation, separate host claims and exactly one spawn per task. |
| A6 | The host chain reads real heartbeat projections through the board snapshot, checks occupancy/weights in JSON and text, and checks the actual runtime and reason in the task spawn comment. |
| A7 | Six `weighted` unit checks, 22 `balancing` regression checks and one real CLI integration check passed during development. The final canonical gate outcome is recorded with the exact candidate in the task result. |

## Qualification boundary

These are model, adapter and isolated process checks on macOS. Structured native quota payloads are fixtures; no provider request, installed fleet cutover or native multi-host workload was run. Missing/stale headroom remains unknown, with neutral ranking; corrupt storage refuses weighted launches. `merge_group` remains the publication gate (R12, §7). All board hosts need schema-2-compatible readers before enabling weights; each intended host receives its own owner-set weights.
