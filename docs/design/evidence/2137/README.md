# Balancing candidate · #2137

Prepared implementation, not a completed task or deployed change. The owner requirements and preference approval are in [#2137](https://github.com/taskqorg/taskq/issues/2137), comments 6560063367, 6560826625 and 6561232067. No live limits, accounts, grants, pins or services were changed.

## Implemented scope

- A1: Balancing replaces the visible Work mode name. The existing `work` route, JSON keys and old M12 anchor remain compatible. Four sections retain detailed project routing and access behind disclosures, with exceptions directly visible.
- A2: strict `run-*` wins for both roles; worker `prefer-*` labels and first matching all-label project rules remain soft. Each subscription candidate retains its own admission checks. Existing conflicting board preferences remain ignored; new-task input still refuses conflicts.
- A3: strict disabled/quota pins retain explicit status blockers. Owner/PM reroute reads back ready-task pins, preserving active/unreleased work and all other labels. No real board reroute was executed.
- A5/A6: host-wide independent quota windows, account rotation, explicit unknowns, capacity separation, native default-account Antigravity observation and serialized due probes reuse existing account/runtime owners.
- A7: worker and reviewer mid-work quota preserve attempt/account/session through restart and resume that same session after the hold. §2.12 accepts the reviewer service-generated quota continuation only after accepted execution verified exited and all executions released. Generic, unbound, stale, unaccepted and unreleased reviewer continuation is not delivered. Unavailable native resume waits without replay or migration.
- A8: host/status quota lines include runtime/account/window/reset or probe. Account quota/probe/recovery telemetry carries matching facts; existing switch/fallback records explain selection. Native provider and telemetry delivery qualification remains unknown.

## Verification

`cargo test --locked --bin taskq balancing_` reproduced two failures before implementation: soft prefer-codex returned one strict candidate; a short reset erased the longer hold. The same checks pass after the fixes. New cases also cover rule ordering/types, disabled/quota status, reroute fencing, overlapping windows, unknown reset, bounded probes, corrupt state and same-session worker/reviewer recovery across restart and unavailable native continuation without replay. The new §2.12 golden trace first failed with an empty reviewer inbox after verified exit, then passed after the guarded model change. Seven traces protect valid continuation and the excluded inputs. `balancing_`: 8 passed; `golden_traces`: 2 passed (fold and admission).

The affected usage-limit/account/golden-admission regressions retain spare/all-account rotation, retry budgets, strict pins, capacity and ownership protections. The account fixture now uses current mock board time rather than hardcoded `1`, which had hidden exhaustion at reset.

`python3 scripts/check.py --lint`: PASS, 0 errors, 276 lines, 55,833 bytes. The existing budget headroom warnings remain.

`node tests/settings-content.browser.cjs`: PASS, 104 light/dark, 320/560px, 18/20px cells, including complete readback, no effect on entry, exact operation destination and Settings Back. [Matrix](matrix.json). An old expected payload omitted the existing Settings origin; the corrected check retains it. Operation selection names admission explicitly after the required section reorder.

Affected Settings/navigation/resource component checks and resource/workspace-route browser checks pass. Resource source integrity now uses the existing build-derived workspace manifest instead of committing a changed source hash; the fixture replay and other asset integrity checks remain.

The rule-ID gate passes with the accepted owner notes. New quota admission semantics live under §4 and the existing linked capacity detail; the original §3 account-list text remains unchanged. The canonical Rust/integration/selftest/Node gate is recorded below when complete. PR merge-result CI remains a publication gate under R12.

## Visual review

Inspected the actual-size dark 320px/18px [Settings](after-settings.png) and [Balancing](after-work.png) captures after the single rebase. Initial repeated project rows displaced the later sections; final disclosures preserve every project detail while all four headings and their direct reviews fit this capture. The matrix checks geometry; it is not native visual acceptance.

## Remaining work

Complete the canonical gate and PR handoff; the reviewer owns merge-result CI. Native subscriptions, OS behavior and telemetry delivery are unverified. Rollback is the unmerged task branch; the record preserves legacy quota aggregate fields, but mixed-version multi-window/probe behavior requires coordinated host adoption.
