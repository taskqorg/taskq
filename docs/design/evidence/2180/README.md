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

## Final checks

Source candidate: `44fb4f74fb9430549774fe4e49652c787a4bd862`; the subsequent documentation amendment records these results only. The single pre-push rebase on `origin/main` was already complete.

- `python3 scripts/check.py --base 4f42f8b44ead036461c0ff37b37910fe91d901db`: **not green**; 900 passed, 7 sandbox-blocked, 42 ignored; 367.419s/900s. All seven protected app-server fixtures failed during configuration setup with `Operation not permitted`. Every progress/lifecycle and snapshot regression passed. The rule gate and pinned contract lint passed; no rule changed.
- `cargo +stable test --locked --test integration`, with the checker's isolated child environment: 22 passed, 1 sandbox-blocked, 1 ignored. The refused case is `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, at `sandbox-exec: sandbox_apply: Operation not permitted`.
- Isolated offline `cargo +stable run --locked -- selftest`: PASS; 130 golden traces, format checks and 8 committed telemetry observations. Board and tool probes remain explicitly unknown. Integration plus selftest took 7.664s.
- All 41 Node/artifact checks from the canonical remainder: PASS, 127.207s. No UI source changed. These local warm macOS timings do not qualify cold CI or other platforms.

Task-fit review: A1's stale-pass/native-time defect is reproduced and corrected at the reader, service, local snapshot and board-consumer boundaries. Implementation review: strict future/invalid guards, bounded reads, quiet work, telemetry/notice deduplication and execution fences remain protected; no dependency, runtime policy or grant was added.

No UI source changed. Installed #2179, native GUI, Windows/Linux event sources and merge-result CI require separate readback. No service, focus, grant, policy or real task execution was changed by this worker. Self-fix publication follows §7 rule 8; this evidence does not claim deployment.
