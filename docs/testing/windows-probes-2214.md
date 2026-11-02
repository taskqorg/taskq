# Windows probe qualification

Task: https://github.com/taskqorg/taskq/issues/2214. Candidate: branch `taskq-2214`, scheduling correction from `e25994dc18ad24b086eb62a0ee8e554524c515c8` plus the native instrument in this deliverable. Windows x86_64 GNU, warm development build, observed 2026-11-02 17:24–17:29 UTC. No installed service was replaced or restarted.

Installed clients: Claude Code 2.1.316 and Codex CLI 0.162.1 (`claude.cmd` and `codex.cmd`). Toolchain: Cargo 1.99.0 (5f94df478), rustc 1.99.0 (b940084d7).

## What the evidence establishes

The host scheduling correction changes a deliberately lowered native profile from CPU class `0x4000`, I/O priority 1 and memory priority 2 to CPU class `0x20`, I/O 2 and memory 5. The existing `host_scheduling` regression test passes and retains above-normal/no-op protection. This establishes the scheduling defect and its correction; it does not establish that scheduling caused the historical process timeouts.

The native instrument calls the production `runtime::probe` with its unchanged 10-second deadline. Each phase makes 30 calls to each installed CLI probe and curl, without printing auth output or dispatching work. Independent `cargo build --locked` runs rebuild TaskQ during these phases (33.39s before; 52.68s after). The build is ordinary worker compilation, not a synthetic disk or CPU saturator. The source mtime was advanced without changing its bytes to force a warm rebuild. These short observations do not reproduce the historical 45-minute load window.

| Probe | Before maximum (ms) | After maximum (ms) | Successful calls per phase |
|---|---:|---:|---:|
| Claude `--version` | 67 | 58 | 30/30 |
| Claude `auth status` | 264 | 233 | 30/30 |
| Codex `--version` | 90 | 90 | 30/30 |
| Codex `login status` | 100 | 95 | 30/30 |
| curl public HTTPS read | 188 | 195 | 30/30 |

Both phases completed 150/150 probes successfully. Harness execution took 19.08s before and 18.76s after. Curl used `https://github.com/robots.txt`, discarded the body and kept a 10-second network limit. Every round also read the native ownership scopes of two live owned command children and fed their actual exit observations through `snapshot::verdict`: 2/2 running in all 30 rounds, in both phases. Cleanup stopped and verified exit of both owned scopes.

`node tests/tray-running.test.cjs` passes the existing host/runtime total, idle, partial, stale, conflicting, duplicate and ended-execution cases. This is separate rendering evidence; the native instrument does not start a board-connected service or qualify production worker/reviewer totals.

## Limits and A1

The reported process timeout was **not reproduced**: the before phase also passed. The prior attempt's 2.70s synthetic timing was below the production deadline and is not failure-before evidence. Scheduling remains a plausible mitigation, not a demonstrated root cause for all 52 historical failures. No causal performance improvement is claimed from these timings.

A fresh project status read during this attempt showed one confirmed Codex worker and a separate unknown Claude execution with reason `Claude original input matches multiple native candidates; inspect them before release`. That ambiguity must remain unknown under R6/R12; scheduling cannot resolve its identity. The native instrument proves continued observability for its two owned children, not that this pre-existing recovery ambiguity is fixed.

A1 is partial: native probes meet deadlines under the observed worker build load, owned native execution observations and existing count rendering pass, and scheduling regression protection remains. Historical timeout causality and a board-connected candidate's worker/reviewer totals remain unverified. No live installation proof is required by §7.8; an isolated service qualification or a reproduction of the historical failure is still needed to close those gaps.

Reproduce the native measurements with the command in [test selection](../testing.md), once per phase, alongside an independent worker build. Run the ordinary `host_scheduling` regression and `node tests/tray-running.test.cjs` for their separate protections. Do not interpret a passing before phase as a reproduced timeout.

## Candidate checks

- `python scripts/check.py --base d76b16d9`: **failed**, 198.188s; Rust reported 876 passed, 49 failed and 45 ignored. The ordinary scheduling regression passed. Later gate steps did not run.
- Forty-seven failed test names match the prior attempt's recorded failure set on baseline `d76b16d9`. The two additional failures were `runtime::app_control::tests::passive_writer_lock_releases_only_confirmed_idle_unheld_locks` and `service::tests::p0_passes_any_focus_but_not_pause_or_limits`. In individual reruns with the checker's isolated test environment, the pipe case passed; the admission case still failed (`Ready` instead of `Doing`). No passing full-suite claim or sandbox-blocked claim is made.
- `cargo +stable run --locked -- selftest`, with a temporary installation and `TASKQ_SELFTEST_OFFLINE=1`: passed 130 fold traces, formats and private telemetry capture. Board/tool qualification intentionally remained unknown.
- `node tests/tray-running.test.cjs`: passed. Both native instrument phases passed as detailed above.

The failed canonical gate and remaining A1 gaps require review; this evidence is not completion or merge authorization (R12, §7).

## Incident telemetry readback

The subsequent investigation recovered the retained incident observations through `taskq profile export --since 24h`. Selection used `observation.started_ms` in `[2026-11-02T16:00:00Z, 2026-11-02T17:00:00Z)` and completed `kind=method` rows. This yielded 9,932 observations across 59 operations. The public table below contains only operation names, counts and elapsed durations; the raw export remains local because it includes unrelated host activity.

| Operation | Completed observations | Timeout observations | Timeout duration range (s) |
|---|---:|---:|---:|
| `runtime.version` | 101 | 20 | 10.000–12.399 |
| `runtime.login` | 100 | 23 | 10.000–14.100 |
| `board.http` | 1,511 | 5 | 65.000–67.688 |
| `link.http` | 138 | 13 | 10.000–45.820 |
| `agent.background-jobs` | 74 | 3 | 0.192–12.183 |

There were no recorded `capacity` failures. The 58 completed `process.cleanup` observations took up to 12.766 seconds; this is asynchronous cleanup duration, not proof of where a probe waited. The timeout range includes inherited observation budgets: the 0.192-second background-job timeout must not be described as exceeding a fresh ten-second deadline. `active_us` is tracing span activity, not CPU usage.

These observations establish actual deadline failures across local CLI and HTTP commands in the same service execution. They do not identify whether time was spent waiting for a process permit, spawning, executing the child, draining inherited pipes or waiting for the observer to be scheduled. The shared CoreQ v0.18.0 `process::execute` implementation uses one deadline for permit acquisition, spawn and output/exit observation, and returns the same `Timeout` after any of those phases. The historical records contain no phase readback. A timeout alone therefore cannot establish I/O starvation or identify the responsible child.

A read-only System event query for 15:55–16:50 UTC found only HTTP service information events, with no recorded suspension, storage error or resource-exhaustion event. The Defender operational query returned one client health report and no scan event in that window. Absence of an event does not exclude contention or scanning.

A fresh board status read separately reported task #2196 (Claude) and this worker #2214 (Codex) as running on Windows. Task #2188 remained unknown with `native observation deferred; cancellation pending`. These are installed-service observations, not qualification of the candidate or an exact host-wide running total. They show that CLI probe failure and execution identity uncertainty must be investigated separately under R6/R12.

The scheduling change remains a tested correction and a possible mitigation. Establishing historical causality requires native process-phase evidence during a recurrence or a reproduction with the original scheduling profile; the current aggregate telemetry and passing before phase cannot supply it. A1 remains incomplete, and the prior failed canonical gate remains unresolved. No additional runtime, permission, host policy or installed-service change was applied during this readback.
