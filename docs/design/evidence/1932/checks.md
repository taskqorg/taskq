# Candidate identity and attributable checks

Worker attempt `6535498100`, 2026-10-31 in Asia/Bangkok. Source is `554552daf48d5b413918d54a8a6d0f88efba4020`; evidence edits do not change application code, tests, dependencies or the injected rules. R12 separates every boundary below.

## Native identity

| Field | Observed value |
|---|---|
| Version/profile | TaskQ 4.6.49; Cargo dev/test, unoptimized; not a release-performance build |
| Embedded revision | `cargo:rustc-env=TASKQ_REVISION=554552daf48d5b413918d54a8a6d0f88efba4020` in this build's output |
| Tools | rustc 1.98.1; cargo 1.98.1; Node v22.18.0; CoreQ 0.14.10 |
| Mac | Mac14,2; 8 CPUs; 25,769,803,776 bytes physical RAM; arm64 |
| OS / WKWebView library | macOS 27.0.1, build 26A434; system WebKit bundle 22625.1.29.11.28 |
| Display | Built-in Retina Display; NSScreen frame `(0,0,1470,956)` points; visibleFrame `(0,0,1470,922)`; backingScaleFactor 2 |
| Display observation | Read-only AppKit NSScreen probe; no display, Dock or OS preference changed |
| Menu capture size | 16 px: 320-point width; 20 px: 360-point width; Home clamp 922 points; Settings content-fit 710/820 points |
| Conversations capture size | 800×600 logical points, 1600×1200 PNG pixels |
| Themes/fonts | Menu/Settings dark/light × 16/20 px; Conversations dark × badge/question/owner/chat × 18/20 px, plus light badge/chat at 18 px |
| Font qualification | Production system UI stack; resolved font-face and VoiceOver attribution Not tested |
| Unsupported/unexercised cells | 1×/external display, other window dimensions, 14 px and full light Conversations matrix Not tested |
| Signature | Identifier `org.taskq.taskq`, Authority `TaskQ Local Signing`, verified by the Cargo runner before execution |
| Menu/Settings/icon binary SHA-256 | `d51ec4f3bcda35e262909eaadf1007141f54b7fb31c96d0e54487c7864559e6a` |
| Conversations binary SHA-256 | `bc5148907e7afef17762db21ac27580bc3c77503947c024aa2d250679895f794` |
| Fresh Conversations reproduction binary SHA-256 | `4ab5cd581eaf5785acc39b6b93f3db53abb12d4a5af58b20584a3738d28679db` |

The three signatures have different signing times; the source, build profile and embedded revision are unchanged. Hashes were read after the respective invocation and before the next signing. The original linker-signed binary before the successful sequential signing had SHA-256 `7f9909b68e20630eb07b61b01f920993fa44112829f26e715e00d0b551bd0325`; it is not the accepted native capture artifact.

These are actual WKWebView snapshot images from the candidate, with off-screen fixture windows and scripted page actions. They exclude native window chrome and status-item placement. They are neither physical screen recordings nor real pointer/IME/accessibility tests.

## Fixture and commands

The isolated installation has no board token; incident reports are off. It contains only invented alpha/beta/gamma records from `docs/design/tray-fixture.sh`. That fixture's SHA-256 is `34a0cda151f2eb6932a8dd3f99120e61665ef85ab2172892db18346ec330cfb2`. It intentionally includes unavailable/stale data; no owner task appears in these images. The data-source timestamp remains unknown rather than silently healthy.

Conversations uses the unmodified `tests/fixtures/conversation-shots.js`, SHA-256 `cdf8adc440470e60ae9f81ced04eeb140d6ecf82d33a70882e1dd0855dd8b124`: invented storefront question #41, two choices, draft and four scripted PM records. The fixture has no source timestamps, so Time unknown is expected. Scripted PM text is not a native provider response.

Reproduction, from the candidate checkout; `fixture` is a newly created disposable OS-temporary directory:

```sh
fixture=$(mktemp -d)
sh docs/design/tray-fixture.sh "$fixture"
TASKQ_INSTALL="$fixture/install" TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --snapshot "$fixture/menu" --route home,settings,icon
TASKQ_INSTALL="$fixture/install" TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --snapshot "$fixture/conversations" --route conversations
```

Run Cargo GUI invocations sequentially. The first `cargo run --locked -- --version` built in 4m54s, but it and an overlapping capture invocation both failed in the shared `taskq.signing` scratch file. Their diagnostics were `replacing existing signature` followed by `No such file or directory`, and `No such file or directory`, respectively. The capture also combined routes that require separate invocations; it never reached route execution. This is failed orchestration, not native success. After the overlap ended, the sequential commands above signed and reached the native renderer. No signing implementation or permission was changed.

## Check results

| Check | Outcome and boundary |
|---|---|
| v2 package and nested baseline hashes | Pass: ZIP bytes/hash; 194 payload entries and 99 baseline entries verified |
| Seven task close receipts and merge ancestry | Pass: [integration.md](integration.md); source integration only |
| `cargo test --locked --bin taskq transcript::tests -- --nocapture` | Pass: 11 tests, 0 failed/ignored, 773 filtered out; 4m48s build, 22.11s tests; source/file-adapter proof |
| 100 MiB reader sample | 108 MiB, 374 backward pages, 9.525846917s including writing; no truncation/refusal assertion changed |
| Other retained reader risks | 10 MiB Claude source; 10,000 variable-height records for both parsers, forward/backward/random-order equality; replacement generation; partial/oversized/shrunk input; native timestamp gaps; redaction |
| `node tests/relative-time.test.cjs` | Pass: explicit units, malformed dates, formatting and source attribution; component boundary |
| `node scripts/build-web.cjs` | Pass; production bundle generation, no source edits |
| `node tests/conversation-times.browser.cjs` | Not tested: exits 1 before launching an engine, `Cannot find module 'playwright'`; no browser result claimed |
| `cargo run --locked -- tray --snapshot … --route home,settings,icon` | Fail, exit 1 after rendering: all eight Menu/Settings cells report `button 6 by 3 px`; F2/#1992 |
| `cargo run --locked -- tray --snapshot … --route conversations` | Exit 0, ten fixture cells captured; visual Fail F1/#1991 at 20 px despite the successful capture command |
| Fresh Conversations reproduction | Exit 0; 18/20 px question PNGs byte-identical to the first run; Send visible at 18 px and absent from the 20 px viewport |
| TCC | No denial/prompt observed; no grant was requested or changed |
| Read-only process probe | `ps`: `operation not permitted`; not used as proof of process exit or UI failure |
| Task filing/readback | #1991/#1992 open, `q-held`, no `taskq` label; no start or PM message |
| `python3 scripts/check.py --base 554552daf48d5b413918d54a8a6d0f88efba4020` | **sandbox-blocked**, not PASS; 78.890s / 900s, no retry. At evidence candidate `43d54c56db1146e9850ef72ae679997174de664b`, selector chose full; rule gate found no changed rule/assertion. Unit driver reported 749 passed/35 ignored; integration 21 passed/1 blocked/1 ignored. |
| Blocked integration | `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, `tests/telemetry_process.rs:189`: `sandbox-exec: sandbox_apply: Operation not permitted` |
| Unreached canonical stages | Offline selftest, later process/component/release stages were not reached; not recorded as passes. The unit log also reports the nested seatbelt fixture sandbox-blocked. No sandbox or guard was weakened. |

The final follow-up commit adds this gate observation only. Application sources, tests, dependencies, candidate native images and environment are unchanged; the full gate is not repeated for an evidence-only update (§7.4, R12).

The reader timing includes fixture generation and page reads together. It is not separate cold/warm I/O, native input p95, frame time, UI long-task cost or DOM/cache usage. Those measurements and the complete hostile fixture list remain Not tested. Q-A6 thresholds remain proposals, not newly adopted performance promises.

## Selected review set

Seven artifacts, one per distinct state/gap. Full theme/size outputs remain disposable; they are not an additional screenshot set. The question 18 px control was inspected to diagnose F1, but its size-only variant is not added to this set. No implementation fix or after-fix acceptance is claimed.

| Artifact | Observation |
|---|---|
| [01 Home 16 px](screens/01-home-16.png) | Explicit unknown source/hosts/counts and stale-source reasons; notification warning visible; F2 guard failure |
| [02 Settings 20 px](screens/02-settings-20.png) | Settings groups and wrapping; all values remain fixture observations, not production service health |
| [03 Owner draft 18 px](screens/03-owner-draft-18.png) | Draft, project/runtime context and reachable Send in the fixture |
| [04 Answer 20 px](screens/04-answer-send-missing-20.png) | F1: answer draft/Attach shown, Send not visible in the viewport |
| [05 PM fixture 18 px](screens/05-pm-fixture-18.png) | Scripted owner/PM text and draft; Time unknown is truthful for missing fixture timestamps |
| [06 Summary unknown](screens/06-summary-unknown.png) | Native renderer raster, 34-point slot size at 2×; unknown counters and incomplete event coverage |
| [07 Summary pause](screens/07-summary-pause.png) | Same renderer's paused Q/pause raster; actual OS slot placement remains Not tested |

No native before/after build comparison or interaction recording was produced. No actual Summary mixed-A/rare/overflow OS-slot case was exercised. Reading the current rasters does not establish those claims.
