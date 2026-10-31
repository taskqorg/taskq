# Native long-history measurement evidence

Evidence for [#2023](https://github.com/taskqorg/taskq/issues/2023); [report](../../../research/2023.md). Product source: `73df356b19e841e3900c06878206bfdd73b7c41e`. This disposable native WebView2 instrument is not installed-product qualification (R12). No product, rule, test assertion, dependency or production state changed (R13, §7.6).

## Boundary and provenance

The temporary Tao/Wry example loads the unmodified built workspace bundle and routes reads off the event loop to the unchanged `transcript::before/after`. Input uses the actual Windows WebView2 CDP target, without a headless browser or platform override. The bridge substitutes registration, status, task history, draft saves and navigation persistence. It does not run `conversations::dialogue`, provider discovery/holder probes, a service, board operations or agent sessions. Its process restart tests the instrument's navigation file, not TaskQ's production preferences or service/session recovery.

[Provenance](provenance.txt) hashes source, lockfiles, workspace bundle, final instrument and controllers. The baseline binary and first runtime driver hashes were not captured before instrumentation edits. The retained [first native source](native-before-missing.rs.txt) is reconstructed from the recorded one-site path-check correction; the [final source](native-instrument.rs.txt) has a captured binary hash. Retained controllers are corrected final versions, not proof of the first runtime bytes. This gap prevents exact-artifact qualification (R12).

Windows 26H2 26300.9711; i5-12600KF, 16 logical CPUs, approximately 32 GiB RAM. Other compilers were active; a point observation showed CPU 10% and 11,270,968 KiB available memory. Primary desktop 2560×1440, WMI current refresh 239 Hz, Windows text scale 140%. Native client 940×740 physical; actual WebView2 viewport 672×529 CSS, DPR 1.4, visible document, Edge 156.0.4314.50, V8 15.6.12.7. Tao reports scale 1.0; neither value proves per-monitor DPI calibration. The report retains the computed font and exact fixture identity.

| Evidence | Content |
|---|---|
| [Baseline](baseline.txt) | Ten cases, fixture hashes/expected keys, raw input/selection/rAF/receive samples, traversal and connected DOM/heap samples |
| [Native adapter](adapter.txt) | Request IDs, cursors, elapsed times, sizes, counts and errors; no message payloads |
| [Adverse](adverse.txt) | Copied-source prepend/append, concurrent input/wheel, shrink/replacement/missing source; incomplete case and controller errors retained |
| [Native trace](native-trace.txt) | Complete browser-scoped trace receipt, numeric renderer events and phase marks; later controller error kept separate |
| [Restart](restart.txt) | Saved anchor, exact owned-process exit/restart, restored offset and ten original source hashes |

The public packet omits loopback WebSocket session URLs and non-renderer trace arguments. Navigation `g` generation values are SHA-256 hashes; raw local scratch remains private. No private filesystem path or credential is included (§7.6).

## Timings and main-thread tasks

Percentiles use nearest rank, `ceil(n × p)`. Each primary input/selection distribution has 40 samples. Timing starts before the CDP request and ends at the next rAF after completion, including transport/host scheduling. It measures synthetic short-draft input and Shift+Left selection, not hardware-to-presentation latency. Synchronous receive durations are separate from entire renderer tasks. Wheel intervals are native rAF callbacks; programmatic edge scrolling additionally tests traversal.

Concurrent wheel/input p95: Claude input/selection 11.3/12.9 ms; Codex 22.8/53.2 ms, 40 samples each. Codex selection maximum: **4495.6 ms**, unattributed to a renderer task. Primary wheel p95 is 6.3–6.6 ms, 518–669 samples/case. WMI's rounded 239 Hz suggests 4.18 ms per nominal refresh; observed rAF misses are not physical compositor presentation times. No hard-coded 16.7 ms label was used.

The complete browser trace contains 23,421 events; the packet retains only numeric renderer events and marks. There are 1759 `CrRendererMain` `RunTask` samples: p95 0.281 ms, max 10.370 ms. Phase windows extend between marks, or to trace end; they include polling/control work, not causal attribution of every task to a page.

| Normal-history phase | RunTask samples | p95 ms | Max ms | >50 ms |
|---|---:|---:|---:|---:|
| Claude prepend | 38 | 3.826 | 8.159 | 0 |
| Claude append | 886 | 0.281 | 9.511 | 0 |
| Codex prepend | 57 | 2.943 | 7.142 | 0 |
| Codex append | 725 | 0.229 | 2.949 | 0 |

This trace covers copied normal 300-record histories after warm-up. Full task traces for 100 MiB/hostile cases remain missing; primary receive and long-task observations are separate. Q-A6's 100/50 ms references were not adopted. The primary observer recorded 341/287 ms maxima in the 100 MiB cases; zero long-task counts elsewhere do not prove all tasks meet 50 ms.

## Reader I/O and bounds

Native reader durations include identity/prefix, open/seek/read and parse, excluding renderer application. Logical range bytes are `end - start`, not physical bytes or read amplification. Generated files were just written; first-open OS-cache state is unknown. Each warm row has 20 sequential identical latest-page requests with unmatched receipt IDs that cannot mutate the UI.

| Provider / source | First-open ms | Logical bytes | Warm p50 / p95 / max ms |
|---|---:|---:|---:|
| Claude normal | 3.248 | 85995 | 2.143 / 2.449 / 4.488 |
| Claude 100 MiB | 13.616 | 1048576 | 10.863 / 14.606 / 22.521 |
| Claude 10,000 | 28.014 | 83297 | 3.741 / 4.365 / 14.870 |
| Codex normal | 5.431 | 100395 | 3.092 / 3.516 / 4.129 |
| Codex 100 MiB | 13.792 | 1048576 | 9.794 / 11.276 / 21.238 |
| Codex 10,000 | 7.315 | 97697 | 4.797 / 5.846 / 13.562 |

OS-cold storage, physical I/O and production contention remain unknown. No shared cache was flushed. The first process has a fresh WebView2 directory; later switches/restarts reuse it.

Both providers reach every expected visible key and `0:0`: 300 normal, 11 long visible messages in each exact 104,857,600-byte source, 10,000 variable-height records and 80 hostile records, with zero missing expected keys. Each 100 MiB source has 100 one-MiB records with ignored padding. Long ignored runs exercise explicit Load earlier. The >16 MiB record is explicitly refused: `transcript record exceeds 16 MiB; view unknown`.

Traversal samples reach 600 mounted items and 2810 connected document elements in both 10,000-record cases. They are sampled maxima, not instantaneous peak guarantees; initial pre-receipt samples do not prove bounds. Heap and native DOM counters include prior fixtures, detached DOM and instrumentation without forced GC. No isolated byte-cache plateau or memory ceiling is proved. Selection/whole-record preservation can exceed the nominal 600-item target.

Hostile inputs include HTML/script-like literals, 16 KiB unbroken identifiers, mixed-direction Unicode, combining characters, emoji, ANSI text, code fences, malformed/ignored lines and an incomplete final record. The hostile execution flag remains false. All ten original source hashes survive; mutations use copies.

Both providers show prepend delta 0.234375 CSS px and older-reader append delta 0, including delayed readback. Earliest-record font changes move at most 0.000016 CSS px. Shrink yields `transcript shrank; view unknown`; same-prefix replacement yields the source-changed/Choose Latest state; disappearance yields `transcript identity unknown`. No unknown becomes healthy empty history (R12).

Final restart readback restores `claude-normal` record `49974:0`, offset 1.953125 CSS px, with delta 0. Both owned process exits are confirmed. This is instrument navigation recovery, not production preferences, service/provider session or OS restart. All nine launched native/controller processes record exit 0. No production process was stopped.

## Instrument failures before corrected checks

These are instrument failures, not fixed product defects (R12, §7.4).

| Failure | Corrected observation |
|---|---|
| Computer Use import timed out twice | No helper input sent; native CDP used |
| Two unavailable Wry/Tao API names | Corrected build passes; WMI refresh reported separately |
| Generated source exceeded 100 MiB | Exact one-MiB records pass size assertion before native launch |
| Negative anchor emitted invalid JavaScript | Parenthesized subtraction; focused restart delta 0 |
| Canonicalization panicked before missing-file adapter call | One-component fixture path guard; unchanged reader returns unknown for both providers |
| Recovery assumed a Latest button existed | Switch away/back; preserve missing-source observation |
| Readiness returned before first receipt, append entered initial read | Wait for content/refusal and observe Windows bridge requests; previously unproved Codex append passes |
| Page-scoped trace completion timed out | One focused browser ReturnAsStream trace completes with 23,421 events |
| Process quit before CDP acknowledgement | Exact owned-child exit is stop receipt; focused stop/restart passes |

Completed baseline measurements were not rerun for faster numbers. Completed Claude adverse observations were reused. Missing/failed evidence was checked after controller corrections. The incomplete Codex case, original timing tails and missing first-pass hashes remain visible; no protection or threshold was weakened.

## Reproduction and delivery

One-shot sources are text evidence, not new product entrypoints or automatic gates: [Rust](native-instrument.rs.txt), [controller](measure.cjs.txt), [adverse](adverse.cjs.txt), [trace](complete.cjs.txt), [restart](restart.cjs.txt). Asserts cover exact sizes, traversal, refusal, trace completion, source hashes and restart anchor tolerance.

On a disposable Windows worktree under `.worktrees/` at the tested source, copy Rust text to `examples/history-2023.rs`; copy controller texts into one OS-temp directory with `.cjs` names. From that worktree run:

```text
npm.cmd ci --ignore-scripts --no-audit --no-fund
npm.cmd run build
cargo build --locked --example history-2023 --target-dir ../../target
node <scratch>/measure.cjs
node <scratch>/adverse.cjs
node <scratch>/complete.cjs
node <scratch>/restart.cjs
```

The controller expects that example binary under `../../target/debug/examples` and finds its built x64 WebView2 loader. Port 19223 must be free; the loopback debugger is child-only. Fixtures, drafts, native directory and logs stay in scratch. The original attempt used `adverse --resume` to retain completed Claude observations; a fresh run needs no flag. Remove only the owned temporary example afterward. Reproduction authorizes no production service operation.

Delivery is prose/text evidence only. The selected canonical gate is `python scripts/check.py --base <rebased-origin-main>`; docs-only selection omits Rust suites and offline selftest (docs/testing.md). See [delivery checks](delivery-checks.txt). The worker does not wait for CI, merge, install or roll out; merge-result CI remains the publication gate (R12, §7).
