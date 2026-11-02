# Native progress sampling clock

Fix for [#2180](https://github.com/taskqorg/taskq/issues/2180), the follow-up to #2173. R6 requires separate lease and execution health; R12 keeps future, invalid, missing and stale observations unknown. No rule or execution authority changes.

## Failure and cause

The task's operator evidence records active #2179 with a fresh owning-host lease, valid UTC native events before wall time, and `native progress time is future or unknown`. This is supplied live evidence, not a new worker observation of the installed GUI.

`Service::observe_progress` supplied `snapshot.observed` to the native reader. That snapshot clock preceded later work in the pass; the board clock itself comes from the last HTTP response's Date. A hot log could therefore have an mtime or native timestamp newer than that bound while still preceding actual UTC observation time.

Before the fix, the existing lifecycle regression was extended to append a valid event after the snapshot. `cargo test --locked --bin taskq progress_health_lifecycle_captures_once_recovers_and_keeps_execution_fenced` failed: `unknown` instead of `active` (`valid source appended after pass start is not future`). The initial missing npm dependency failure occurred before test execution and is not reproduction evidence.

## Correction and retained protections

- Sample local UTC after obtaining source metadata and again after the bounded incremental read. Publish that clock as `progress.observed`; do not use pass-start or cached board time to fence native events.
- Re-age local heartbeat/progress with the local sample time from the exact retained local execution. Historical exited observations cannot supply a new sampling clock. Do not renew the earlier executor or board lease observation.
- Keep strict future mtime and event checks, invalid-timestamp fault retention, file identity/replacement checks, partial-read handling, tool deadlines and quiet-work behavior. There is no grace interval or healthy fallback.
- Deduplicate progress stalls by their existing `overdue_since`, retaining the episode through temporary source errors until real recovery. A filtered run after the clock correction exposed a repeated notice after service reconstruction (`1` instead of `0`); subtracting a fresh progress age from pass-start time had shifted the dedup key.
- Keep the existing timestamp-fault assertions over every field except the newly sampled `observed` timestamp. They still require unchanged unknown status, source, last-event null and diagnostic through elapsed time, empty reads and unrelated events; only a valid turn boundary recovers consumed invalid data.

## Verification boundary

The reader regression advances UTC across an append between metadata capture and event consumption without sleeps. It checks the exact event/observation times, rejects genuinely future metadata without advancing the cursor, then consumes the valid event incrementally after metadata correction. Existing reader cases retain invalid/future event fencing, deadlines, concurrent tools, partial writes and replacement.

The lifecycle case uses actual event files, an invented board clock and the existing collector. It covers source data newer than pass-start, local warning clearing, board heartbeat and remote snapshot projection, overdue/recovery capture, repeated passes with different sampling offsets, restart deduplication and unchanged attempt/execution fences. Its fixture refuses any replay or termination.

The development filter `cargo test --locked --bin taskq progress` passed all 9 selected checks, including the lifecycle reproduction and all five reader checks. The final canonical run also passed their updated assertions, including rejection of an mtime fallback when UTC reverses during the read.

## Timestamp-fix checkpoint

Source candidate: `44fb4f74fb9430549774fe4e49652c787a4bd862`; the subsequent documentation amendment records these results only. The single pre-push rebase on `origin/main` was already complete.

- `python3 scripts/check.py --base 4f42f8b44ead036461c0ff37b37910fe91d901db`: **not green**; 900 passed, 7 sandbox-blocked, 42 ignored; 367.419s/900s. All seven protected app-server fixtures failed during configuration setup with `Operation not permitted`. Every progress/lifecycle and snapshot regression passed. The rule gate and pinned contract lint passed; no rule changed.
- `cargo +stable test --locked --test integration`, with the checker's isolated child environment: 22 passed, 1 sandbox-blocked, 1 ignored. The refused case is `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, at `sandbox-exec: sandbox_apply: Operation not permitted`.
- Isolated offline `cargo +stable run --locked -- selftest`: PASS; 130 golden traces, format checks and 8 committed telemetry observations. Board and tool probes remain explicitly unknown. Integration plus selftest took 7.664s.
- All 41 Node/artifact checks from the canonical remainder: PASS, 127.207s. No UI source changed. These local warm macOS timings do not qualify cold CI or other platforms.

Task-fit review: A1's stale-pass/native-time defect is reproduced and corrected at the reader, service, local snapshot and board-consumer boundaries. Implementation review: strict future/invalid guards, bounded reads, quiet work, telemetry/notice deduplication and execution fences remain protected; no dependency, runtime policy or grant was added.

No UI source changed. Installed #2179, native GUI, Windows/Linux event sources and merge-result CI require separate readback. No service, focus, grant, policy or real task execution was changed by this worker. Self-fix publication follows §7 rule 8; this evidence does not claim deployment.

## Manager status extension

[Owner steer 6576533840](https://github.com/taskqorg/taskq/issues/2180#issuecomment-6576533840) adds the separate actionable `Stale heartbeats` block and explicitly authorizes R6/status-report changes. The root contract and linked report specification were updated before implementation. Amended rule: **R6**.

The block selects only existing Doing/Review `heartbeat.warning` values. It preserves the same typed JSON/native projection, clock provenance, exact attempt/execution identities, source observations and ages, source diagnostics and one safe inspection action. It appears after Questions/Decide, omits healthy quiet work and empty results, and removes duplicate entries from the generic `Stuck:` list. No JSON key, threshold, detector, manager action or permission was added.

- Before: the new manager golden check failed because ordinary status contained no `Stale heartbeats:` block. After: the same check passes, including stale host with active executor, missing host, source error, overdue tool with fresh lease, unknown executor, awaiting execution, stale progress, recovery/empty output, preserved Questions/Decide, long source-metadata links and explicit future age. The golden is [stale-heartbeats.txt](../../../../tests/golden/stale-heartbeats.txt).
- Actual candidate `cargo run --locked -- status` succeeded against the configured board. The [initial capture](status-cli-warning.txt) exposes #2183's fresh lease/active executor separately from missing native progress, with runtime, host, current attempt/execution, observations and a safe inspection action. Questions and the completed result requiring discussion precede the block; there is no duplicate Stuck entry. That capture precedes the presentation-guard corrections below; its selection/discovery path is unchanged, while the final field presentation is qualified by the golden.
- A [fresh final-renderer capture](status-cli.txt) shows #2183 in Decide as a confirmed result and no remaining Doing/Review health warnings; the health block is correctly absent and Questions/Decide remain discoverable. These are ordinary manager status reads, not web fixtures or an inference from UI health. No manager was launched and no action was taken on #2183/#2184.
- The rule gate initially refused R6 because it omitted accepted owner steering from task text, contrary to the binding inbox notes in §2. A Python regression failed before the fix, then passed. The gate now includes only trusted, unedited plain owner `steer` accepted in Doing with matching revision/operation identity; attempt-bound, stale, duplicate, reviewer, ignored-state and unknown histories cannot add approval. Existing answer/selected-option protections remain. The actual R6 gate now passes using this task's owner note, without a second approval.
- The existing historical unaccepted-Exit regression retains its state, lease, input and execution assertions; its human-report check now requires the actionable health block instead of duplicate Stuck text. Structured JSON/PM consumers already receive `tasks[].heartbeat`; no second health collection was needed.
- The first expanded canonical run found two presentation defects: the new executor line leaked a raw execution diagnostic protected by the existing owner-text guard, and `Problem:` was correctly rejected by the production-card guard. The renderer now reports executor state/source/observation and calls the shared health diagnostic `Reason:`. Both original guards pass unchanged, as does the manager golden. The failed expanded run was 899 passed, 9 failed (7 sandbox refusals plus these two defects), 42 ignored; 291.974s. It is not final acceptance evidence.

## Final amended-R6 verification

Code candidate: `4ffb466141ada2107705fffd762150033cfe5fd1`; the final amendment adds these results only. No further rebase followed the first push.

- `python3 scripts/check.py --base 4f42f8b44ead036461c0ff37b37910fe91d901db`: **not green locally**; 901 passed, 7 sandbox-blocked, 42 ignored; 164.077s/900s. The seven failures remain the protected-fixture configuration setup refusals. Clock/lifecycle, manager golden, historical unaccepted Exit, original owner-text and production-card guards all pass. The actual owner-steer R6 rule gate and pinned contract lint pass; root contract: 294/300 lines, 59,077/61,440 bytes, longest line 394/400 characters.
- Isolated `cargo +stable test --locked --test integration`: 22 passed, 1 sandbox-blocked, 1 ignored; same `sandbox_apply: Operation not permitted` case. Isolated offline `cargo +stable run --locked -- selftest`: PASS, 130 traces and 8 committed telemetry observations; board/tools deliberately unknown. Combined remainder: 14.125s.
- All 41 Node/artifact checks ran again and passed in 90.320s. Final canonical plus independent remainders: 268.522s on this warm macOS host, not a cold-CI qualification.
- The final-renderer actual text read omits an empty health block and keeps Questions/Decide. A subsequent actual `target/debug/taskq status --json` read reports #2180 as fresh host observation, active executor, quiet progress and null warning; all Doing/Review rows in that read have null warnings. These are separate fresh reads, not one atomic snapshot. The typed fields already support manager discovery without another collection or threshold.

Task fit: A1's false-future reason and the accepted owner status extension are implemented and verified at their stated boundaries. Quality: no new dependency, health detector, threshold, grant, capacity or execution control; native future/invalid, owner-text/card, lease/input and notice protections remain. Eight host checks remain sandbox-blocked, and merge-result `merge_group` CI is unverified. Installed #2179 and Windows/Linux source requalification were not run; this worker made no deployment.

## Merge-queue Windows fixture correction

The [merge-group run 41451314824](https://github.com/taskqorg/taskq/actions/runs/41451314824) tested merge result `d29d60388abfd15075a78a4ed79524b31c088b3c`. Linux, macOS and the GNU Windows loader check passed. The Windows Rust suite failed only `resources::tests::local_capacity_receipts_never_enter_the_remote_exchange`: 900 passed, 1 failed, 43 ignored; selected gate 666.828s/900s. All progress and manager-health tests passed there.

Failure before: the fixture's final flush reported `telemetry flush unacknowledged (accepted 24 committed 20 lost 0 failed 0 batches 8 append_us 18474693 max_append_us 4104803 inflight 4)`. This establishes unacknowledged in-flight telemetry, not data loss or a health regression. The pinned CoreQ flush waits 3s; one observed append took 4.105s. The reason for that Windows append latency is unknown. An unchanged local macOS run passed in 0.28s, so this host did not reproduce the Windows timing failure.

The collector is now scoped to the local capacity save that requires it. Its existing guard settles asynchronous mandatory receipts before the unchanged flush assertion; the five remote exchange refusal checks then run without collecting unrelated failed-method diagnostics. Every receipt-isolation, unchanged-policy, refusal-count and local-operation identity assertion remains unchanged, including capacity, restart, stop, uninstall and install. No product code, telemetry deadline, retry, dependency, permission or accepted rule changed in this correction (§7 rule 3; R12).

The affected regression and canonical gate results below qualify the local candidate; only a new Windows merge-group run can establish that platform's pass after this correction. Prior actual manager status/JSON readbacks remain evidence for unchanged health code.

Pass after, local macOS: `cargo test --locked --bin taskq local_capacity_receipts_never_enter_the_remote_exchange` passed (1 test, 0.15s execution), using the checker's isolated child environment. The unchanged fixture passed locally before the edit too; the supplied Windows failure is the before evidence, and no Windows pass is claimed.

The single resumed canonical run, `python3 scripts/check.py --base 4f42f8b44ead036461c0ff37b37910fe91d901db`, is **not green locally**: 901 passed, 7 sandbox-blocked, 42 ignored; 263.311s/900s. All seven failures are the same protected app-server fixture setup `Operation not permitted` refusals. The corrected isolation test, clock/lifecycle/dedup regressions, manager golden, original owner-text/card guards, rule gate and contract lint pass. The gate stopped before integration, offline selftest and Node remainders; their prior evidence above is reused for unchanged product sources, checks and dependencies. No second full run or rebase followed this publication-failure requeue.

Task-fit review: A1 and owner steer 6576533840 remain covered by the unchanged health implementation and manager discovery evidence. Implementation review: this correction changes only fixture collector lifetime and documentation; all H-A2 assertions and the flush assertion remain. Windows pass-after and new merge-result CI remain unknown; installed/native source requalification and deployment were not performed. Amended rule for the complete PR remains R6; this correction amends no rule.


## Main-moved conflict recovery

The service requested a `main-moved` requeue for PR #2185. The authorized rebase uses `origin/main` at `50034cbaf6f09480c98732cf4b04ba951481c4ca`. It required one manual resolution in `Service::new`: retain the incoming balancing fields `last_spawn` and `headroom_error` together with the native `progress_clock`. `git range-diff` confirms the other two commits are unchanged. Production-source candidate: `efaef9f618e77dfe128f4f82d50cda18b8df6a0d`.

Failure before: the combined contract had 301 lines, exceeding R8's unchanged 300-line cap; the first selected gate stopped at lint in 3.711s, before Rust checks. The R6 paragraph was compressed without removing its output order, sources, omissions or linked formats. Pass after: the same lint reports 300/300 lines, 60,842/61,440 bytes and longest line 394/400 characters. The rule gate recognizes only the authorized R6 change. No other decision or guarding assertion changed during conflict recovery.

The rerun `python3 scripts/check.py --base 50034cbaf6f09480c98732cf4b04ba951481c4ca` is **not green locally**: 912 passed, 7 sandbox-blocked, 42 ignored; 285.378s/900s. All seven failures are the same protected app-server fixture setup `Operation not permitted` refusals. The clock/lifecycle/dedup checks, manager golden, resource-isolation fixture, original owner-text/card guards and incoming balancing checks pass. Rule gate, contract lint, Python checks and web build pass. The canonical command stops at these unit refusals; remaining checks are run separately below rather than reporting them as passed by that command.

Fresh ordinary candidate CLI reads both succeeded. The [text capture](rebase-status-cli.txt) preserves Questions and completed-result Decide rows before the actionable block; #2176 has a fresh lease but an explicitly unknown executor and missing progress, with the safe inspection step and no duplicate Stuck entry. #2180 has no warning. The separate [typed health capture](rebase-status-health.json) confirms #2180's current attempt, fresh host observation, active native progress and null warning; #2176 carries the same unknown warning used by the text renderer. These are separate board reads, not an atomic snapshot or native GUI proof. No task/manager action was taken from these observations.


The separately completed post-rebase remainder ran the same candidate sources: integration **23 passed, 1 sandbox-blocked, 1 ignored**; the refusal remains `telemetry_process::resume_records_its_result_without_releasing_the_host_hold` (`sandbox_apply: Operation not permitted`). Offline selftest passed 130 traces, format checks and 8 committed observations in its isolated store; board/tools remain unknown. All 41 Node/artifact checks passed. Combined remainder elapsed: 98.236s; no retry. This completes the observations left pending when the prior worker exited, without rerunning unchanged tests.

A fresh [resumed-attempt status capture](resume-status-cli.txt) also succeeds and discovers #2180's current attempt `6579128827` in `Stale heartbeats`, after Questions/Decide, without duplicate Stuck output. The installed producer reports fresh ownership and an active executor but unknown/future native progress. The candidate CLI faithfully retains that producer diagnostic; this read does **not** establish native pass-after or identify the installed service's source revision. Earlier clearing observations remain historical. No worker installation or service change was performed.

Final scope review: A1's timestamp correction and the owner-authorized R6 manager projection remain covered by the recorded failure-before/pass-after source regressions. The rebase retains incoming host-balancing fields and every execution fence; only the authorized R6 paragraph was compressed to satisfy the unchanged R8 cap. The local gate remains not green because eight host checks are sandbox-blocked across unit/integration suites. Windows pass-after, installed native qualification and merge-result CI remain unknown.
