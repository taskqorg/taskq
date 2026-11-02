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
