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

The rule-ID gate passes with the accepted owner notes. New quota admission semantics live under §4 and the existing linked capacity detail; the original §3 account-list text remains unchanged. PR merge-result CI remains a publication gate under R12.

### Final local gate

The canonical command `python3 scripts/check.py --base 0d050c51c2e1102c4b1a8d7af401f9beb362e54b` stopped at Rust unit checks after 365.469 seconds (900-second budget): 843 passed, 8 failed, 40 ignored. Seven protected app-server fixtures could not create their temporary `.codex/config.toml`: `PermissionDenied`, `Operation not permitted`. These are **sandbox-blocked**, not PASS. A separately isolated reproduction confirmed the same fixture setup refusal.

The eighth failure expected the old generic `steer attempt` header. The accepted quota continuation now writes `steer quota attempt`; the corrected assertion retains the same attempt, retry, no-`gone` and original-account hold protections. The same `usage_limit` filter passes afterward: 5 passed. Final `balancing_`: 8 passed. Full-suite success remains unclaimed; no sandbox restriction was weakened or bypassed.

The downstream checks that the stopped gate did not reach ran separately against the final source:

- `cargo test --locked --test integration`: 22 passed, 1 ignored, 1 **sandbox-blocked**. The existing read-only telemetry case refused nested `sandbox-exec` with `sandbox_apply: Operation not permitted`.
- `TASKQ_SELFTEST_OFFLINE=1 cargo run --locked -- selftest`, in a temporary install: PASS, 120 golden traces, formats and 8 committed staged telemetry observations. Board and native tools remain explicitly unknown.
- All 37 `tests/*.test.cjs` commands, README shot check/update instruments and release-tag/exact-source release instruments: PASS (92.8 seconds for this supplemental selection).
- Final `tray-resources`, `workspace-route` and `settings-content` browser instruments: PASS; Settings retains all 104 cells.
- Final contract lint, rule-ID gate and whitespace checks: PASS.

A final terminology readback found the legacy editor's `Edit work mode` button. After changing it to `Edit Balancing`, the resource component check, fixture render assertion and final browser selection pass. The isolated legacy renderer was also visually inspected. The usage-limit documentation now points to same-session recovery rather than the superseded retry behavior.

## Visual review

Inspected the actual-size dark 320px/18px [Settings](after-settings.png) and [Balancing](after-work.png) captures after the single rebase. Initial repeated project rows displaced the later sections; final disclosures preserve every project detail while all four headings and their direct reviews fit this capture. The matrix checks geometry; it is not native visual acceptance.

## Remaining work

Local host restrictions leave eight fixture checks sandbox-blocked; the reviewer owns exact-candidate merge-result CI and publication. Native subscriptions, OS behavior and telemetry delivery are unverified. Rollback is the unmerged task branch; the record preserves legacy quota aggregate fields, but mixed-version multi-window/probe behavior requires coordinated host adoption.


## Review corrections

The review of `4896de1a` found three gaps. These corrections retain the accepted A1–A8 scope and the existing ownership, strict-pin, account and retry protections (R8/R12/R13, §5, §7).

- **A1, before:** `node tests/tray-resources.test.cjs` failed at `Home Balancing false: Task routing`. Home's workspace handoff reached a separate resource editor with none of the four sections. **After:** Home, Settings and the workspace resource review share the same four-section renderer. The component regression and actual Home click/handoff browser checks pass; opening changes no policy. Existing host meters/editor, advanced access/routing details and project controls remain available.
- **A3, before:** `cargo test --locked --bin taskq golden_traces` failed because a spawn concurrent with reroute entered Doing. **After:** durable routing intents reserve a board-ordered fence without changing task state. Matching completion releases it; every later claim binds the routing revision. Golden traces cover concurrent/stale claims, competing intents, active attempts, correct completion and legacy unfenced audits. CLI/service regressions also retain strict pins for an earlier winning spawn, reconcile interrupted writes, refresh stale selections and preserve retries. Receipts derive state from the final log rather than assuming Ready. Unknown writes retain the intent for automatic service reconciliation. No real board reroute was run.
- **A5/A6/A7, before:** `cargo test --locked --bin taskq balancing_quota_write_failure` failed at `another project must not select the exhausted default`. **After:** write-ahead quota observations and a shared in-process failure fence prevent admission from every project until persistence succeeds. Regressions force an aggregate-save failure after a valid read, retain the exhausted account and spare, replay a journal with process memory removed, and refuse starts when the journal itself cannot be written. Repeated identical observations retain the prior no-op behavior. New launches also require writable quota storage; limits/accounts/grants are never rewritten.

The affected Balancing filter passes 11 tests; usage-limit passes 5. These are model/service instruments, not native provider proof. The final candidate gate and its remaining gaps are recorded below.

### Follow-up visual inspection

Inspected the actual Home-to-workspace destination at 280px/20px in both themes: [dark top](home-balancing-dark-top.png), [dark bottom](home-balancing-dark-bottom.png), [light top](home-balancing-light-top.png), [light bottom](home-balancing-light-bottom.png). All four sections, meters, editor and project controls are present in the shared scroll; large text wraps without horizontal overflow. The existing operation-review title and navigation are retained. The eight resource-browser cells include four actual Home handoffs, and the Settings instrument retains 104 cells. Screenshots and synthetic IPC do not qualify installed/native execution.

Compatibility: all board hosts must adopt routing-fence support before reroute is used. Older folds do not enforce it. No deployment or live policy changes were performed.


### Current check boundary and contract decision

The follow-up canonical gate on the earlier frozen source stopped after 459.231s: 844 unit tests passed, 10 failed, 40 were ignored. Seven failures were the same protected app-server temporary `.codex/config.toml` sandbox refusals (`PermissionDenied`, `Operation not permitted`). Three service failures exposed test contamination: the recovered-quota fixture used the shared unwritable `/nonexistent` host store, so its correctly retained failure fence affected later fixtures. The fixture now has an isolated writable temporary store; no protection assertion was removed.

Final source `4b9c4eaa` scoped readback: `cargo test --locked --bin taskq service::tests` passed 139, ignored 4; `balancing_` passed 11; `golden_traces` passed 2. The three previously failing service cases pass in that combined service run. Contract lint and rule-ID checks pass. The final CLI regression also preserves dependency blocking during pending-intent reconciliation and every receipt after a partial comment-read failure. The entire final canonical gate has not rerun; its PASS remains unknown.

**Decision resolved (R13, §8/§9).** [Owner answer 6561948437](https://github.com/taskqorg/taskq/issues/2137#issuecomment-6561948437) approves the coordinated cutover for routing fences and reviewer quota input. The root contract and configuration now require compatible readers on every board host before new entries, preserve active execution and quota ownership, and require compatible rollback without deleting evidence. This records the accepted A3/A7 boundary; it grants no live upgrade, policy or account change. Native provider/OS/telemetry proof and merge-result CI remain unknown.


### Quota evidence through update cleanup

The existing `platform::tests::sweep_keeps_the_allowlist_and_live_releases` regression now includes the pending quota journal and quota writer lock in its byte-preserved files. Before the fix, `cargo test --locked --bin taskq sweep_keeps_the_allowlist_and_live_releases` failed with `update lost accounts-state.pending.json`. The same command passes after adding both files to the existing install allowlist. Every prior owned-file, release, token, policy and native-recovery assertion remains. This closes an A7 restart/update gap: cleanup must not delete a failed quota observation or replace the shared lock identity. No installer or service was run against the live host.


## Approved final candidate

Source `658994031bb976fee221e8acc87d4df4bbab8358` includes the approved §8/§9 cutover and the update-sweep fix. The final canonical command `python3 scripts/check.py --base 0d050c51c2e1102c4b1a8d7af401f9beb362e54b` completed its Rust unit selection after 189.825s of the 900s budget: **847 passed, 7 sandbox-blocked, 40 ignored**. All service, routing, quota, golden and update-sweep cases passed. The seven protected app-server fixtures still refused temporary `.codex/config.toml` creation with `PermissionDenied: Operation not permitted`. The gate stopped there and is **not PASS**; those checks are not product failures or qualified native results.

The downstream selections ran separately on the same source:

- `cargo test --locked --test integration`: **22 passed, 1 sandbox-blocked, 1 ignored**. The remaining case refused nested `sandbox-exec` with `sandbox_apply: Operation not permitted`.
- `TASKQ_SELFTEST_OFFLINE=1 cargo run --locked -- selftest`: **PASS**, 126 golden traces, owner-text formats and 8 committed staged telemetry observations. This used a temporary installation with no board read or native tool probe, through the canonical `test_environment` helper.
- All 37 `tests/*.test.cjs` checks: **PASS**. README shot check/update and release-tag/exact-source release instruments: **PASS**.
- Contract lint and rule-ID gate: **PASS**, including the accepted §8/§9 edits; `taskq.md` has 278 lines, 56,359 bytes and zero lint errors. Existing budget-headroom warnings remain.
- The resource and Settings browser evidence above remains source-identical: no JavaScript, CSS, HTML, browser-test or package inputs changed after its 8 resource cells, 4 Home handoffs and 104 Settings cells passed. No visual recapture or native acceptance is claimed from the Rust/docs-only follow-up.

No live policy, account, grant, task pin, installation or service was changed. Eight host fixture checks remain sandbox-blocked. Native providers, OS behavior and telemetry delivery remain unqualified; merge-result `merge_group` CI and reviewer publication remain required by R12/§7. The coordinated cutover is documented, not executed.
