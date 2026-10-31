# Resumed native checks · #1932

Attempt `6536345627`. R12 and §7 separate source, renderer, real native transport and platform acceptance. The [report](../../../research/1932.md) owns the current verdict; [historical checks](checks.md) retain the original failure evidence.

## Identity

- Source/build: `67f729e4dc35d20f044efe8cb7a3b6a939c5e048`, version 4.6.50, Cargo dev/unoptimized. Embedded `TASKQ_REVISION` equals that SHA. Application/test/Cargo/contract diff against main `cbeb8db913aa0908fbe9490e7de8404e7a306231` is empty.
- macOS 27.0.1, build 26A434, arm64; system WebKit 22625.1.29.11.28. Existing host: Mac14,2, 8 CPUs, 25,769,803,776 bytes RAM. This attempt's [AppKit readback](native-environment.json) records screen 1470×956 points, usable area 1470×922 and backing scale 2. No display preference changed.
- The system font API returns `.AppleSystemUIFont`; computed input stack is `-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif`. Native inputs measured 16/20 px; question hit-tests cover 18/20 px. Per-glyph fallback inspection, physical IME and other display scales remain Not tested.
- [Manifest](resume-manifest.json) pins signed product binaries separately per run, production workspace bundle, fixture/instrument files, ten selected images and two clips. Product binaries use identifier `org.taskq.taskq`, TaskQ Local Signing via the Cargo runner. The disposable instrument bundle is also signed with TaskQ Local Signing and passes `codesign --verify --strict`.
- The production workspace bundle SHA-256 is `e4ebb95ecf37f063832e98136db5b89379d0991b98693a89d6482a16893f846a`. The UI30 ZIP checksum and all 194 payload hashes were reverified. Original prototype references are not after-state.

## Attributable commands

Commands run from the task worktree. `fixture` below denotes a new disposable OS-temporary directory, not a production installation. Incident reporting is off; the invented fixture has no board token/service. Native Cargo commands ran sequentially.

```sh
node scripts/build-web.cjs
TASKQ_INSTALL="$fixture/install" TASKQ_INCIDENT_REPORTS=off cargo build --locked --bin taskq
sh docs/design/tray-fixture.sh "$fixture"
TASKQ_INSTALL="$fixture/install" TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --snapshot "$fixture/menu" --route home --route settings
TASKQ_INSTALL="$fixture/install" TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --snapshot "$fixture/conversations" --route conversations
swift docs/design/evidence/1991/capture.swift "$PWD" "$fixture/composer"
```

| Check | Actual outcome / boundary |
|---|---|
| Candidate build and web bundle | Pass; dev/unoptimized, no production install. Existing compiler warnings retained. |
| Explicit Home/Settings native capture | Pass, exit 0, exactly eight PNGs: dark/light × Home/Settings × 16/20 px. The unchanged overflow guard has no diagnostic; unread 6 plus warning retained. |
| Conversations native capture | Exit 0; ten original fixture cells. This is capture success, not all-flow acceptance. |
| `swift docs/design/evidence/1991/capture.swift ROOT OUT` | Pass, all six WKWebView cells: dark/light 18/20 px at 800×600, plus dark/light 20 px at 350×600. Send/Attach/help hit-tested; textarea client=scroll height. |
| Supported isolated interactive `cargo run --locked -- tray` | Refused: `taskq tray already runs on this host`. No lock bypass or production tray/service change. |
| `node tests/conversations-flow.browser.cjs` | Pass: invented worker steer, immutable unknown recovery, explicit follow, long draft, correlated PM/clarification/result component chain. No live task/provider input. |
| `node tests/conversations.browser.cjs` | Pass: source/browser CW1–8, drafts/caret/files, archive/restore, search, IME fixture, immutable IDs, partial delivery, restart/close and narrow layout. These are browser fixtures, not real native effects. |
| `node tests/worker-panel.browser.cjs` | Pass: local/remote fixture panels, append, attempt fence and teardown. No live two-host proof. |
| `node tests/reply-files.test.cjs` | Pass: image/document payload, file-only send, immutable operation retry and revision fencing. No real attachment delivery. |
| `node tests/relative-time.test.cjs` | Pass: source units, malformed times and formatting. |
| `node tests/tray-answer.test.cjs` | Pass: exact IPC, foreign receipt, timeout, same-ID and stale/lost-response fixtures. |
| `node tests/notifications.browser.cjs` | Pass: four Chromium dark/light × 16/20 px cells, native-platform flags, exact count/warning, unchanged clipped-label negative guard, race/clear/question preservation. |
| `node tests/conversation-times.browser.cjs` | Partial: Chromium 12 width/font/theme cells pass; command then exits 1 because Playwright WebKit executable `webkit-2370/pw_run.sh` is absent. WebKit-browser matrix Not tested, not a failure of the product WKWebView. No browser was installed outside the allowed folders. |
| Canonical gate | **sandbox-blocked, not PASS**: 241.006 s / 900 s, no retries; details below. |

The combined `--route home,settings,icon` invocation exited 0 but produced only 36 icon/app rasters and **no** Home/Settings PNGs. It is not counted as an eight-cell pass. The explicit repeated-route invocation above produced all eight files and exercised the guard. Cause of the combined-route discrepancy remains uninvestigated; no product fix is inferred from that observation.

## CUA native interaction

[interaction.swift](interaction.swift) is a resizable native WKWebView instrument over the production bundle, with a nonpersistent web store and no native IPC bridge. Its native menu selects question/history fixtures, themes and fonts, records snapshots, measures and quits. Standard Edit actions are necessary for WKWebView keyboard paste/select-all; their initial absence was an instrument defect, corrected before the frozen run. `history-state.js` wraps the existing seeded fixture body from `tests/conversations-render.browser.cjs`; its assertion requires exactly 10,000 visible source records. The Chromium-only long-task observer is removed because this WKWebView does not support that entry type; the metric remains unknown.

The instrument was compiled with `swiftc docs/design/evidence/1932/interaction.swift -o <disposable-app>/Contents/MacOS/renderer`, given `ProbeRoot`/`ProbeOut` bundle keys, signed, then opened and operated through CUA. It also accepts explicit root/output arguments for a local instrument run. The output and source/binary hashes identify the frozen run; exploratory output is not substituted.

Observed action sequence:

1. Open question at dark/20 px, 800×600; start recording. Native wheel down over question reveals its choices. Paste Thai/Japanese text; Shift+Enter; press physical x under the current Russian layout, select/copy the resulting `ч`. IME composition count remains 0; no IME pass.
2. Replace with 60 Unicode lines. Measurement: input client/scroll height equal, resize none, Send remains inside viewport. No answer is clicked or sent.
3. Switch to light/20 px history fixture and explicitly select its disposable owner row. Native wheel up/down pauses follow. Paste/replace the independent draft, add a line, retain it unsent.
4. Traverse backwards with native wheel input. At the long 12-page ignored-only run, the existing fill budget exposes Load earlier. It is visible but absent from CUA's AX text at that observation; a screenshot-coordinate click continues it. This is not a complete accessibility pass. Native wheel traversal then reaches the first record.
5. Observations show `seen=15000`, `duplicate=false`, `mountedMax=600`, first key `0:0`, offset 34. OS zoom changes actual viewport 800×600 → 1470×890; first key/offset remain `0:0` / 34. The draft remains. Explicit Jump to latest removes paused-follow state. No JS `scrollTop` assignment drove this native traversal.
6. The deliberately missing native-save receipt produces unknown/save recovery copy, keeps the draft and disables Send. No save/answer/provider receipt is fabricated. Quit only the disposable instrument; CUA reports `App quit`.

[Raw observations](native-observations.jsonl) contain source-time-stamped measurements, action counts and sink operation identities. They contain only invented records. The history fixture models page reads with seeded delays; it is not actual file I/O, a provider session, worker execution or the application's native IPC.

## Measurements

| Sample | Observed | Limits |
|---|---|---|
| History source / traversal | 10,000 message source records; 15,000 message/tool items seen; sampled mounted max 600; no mounted duplicate; beginning reached | Claude-shaped simulated backend, existing seeded fixture. Other hostile/native-source cases and JS cache/process-memory plateau remain Not tested. |
| Input feedback | 4 history input events, p50 6 / p95 9 / max 9 ms | Native input-event → next animation frame; tiny sample, not time from physical keypress. |
| Receive handler | 105 calls, p50 28 / p95 34 / max 38 ms | Synchronous `taskq.receive` duration in existing fixture; not complete paint cost or main-thread long-task measurement. |
| Wheel-adjacent frames | 1,690 intervals, p50 17 / p95 24 / max 62 ms | Intervals in the 500 ms after native wheel events; samples stop after 120 s. Capture at 2 fps adds load. No claim of display-budget compliance. |
| Resize anchor | `0:0`, 34 px before and after 800×600 → 1470×890 | One settled case; not the full Q-A3 matrix. |
| Long question draft | 60 lines, textarea client=scroll, resize none, Send in viewport | No real attachment or answer submission. |
| >50 ms main-thread tasks | Not tested: `longtask` unavailable in this WKWebView | Receive handler duration cannot replace this metric. |
| Cold/warm 10/100 MiB I/O and cache | Not tested | Earlier source-reader timing includes writing and reads together; it is not cold/warm qualification. |

Q-A6's p95 ≤100 ms and >50 ms main-thread thresholds remain proposed reference-host targets. No performance decision changed (R13).

## Review set

Historical selection for this earlier checkpoint. The [current ten-screen selection](isolated/README.md#acceptance-and-review-set) replaces 03/05 with actual isolated application frames; all other frames below remain selected.

Exactly ten representative PNGs. Original seven retain source `554552da`; images 08–10 use the resumed candidate. The manifest names source and SHA-256 for each. All were inspected at usable size; the original seven were also revisited in this attempt. Source/reference images are not promoted to current native after-state.

| Screen | Scope |
|---|---|
| [01 Home before](screens/01-home-16.png) | Valid F2 before: separate warning, failed overflow guard. |
| [02 Settings before](screens/02-settings-20.png) | Valid F2 before; wrapping and fixture-only facts. |
| [03 Owner draft](screens/03-owner-draft-18.png) | Historical native draft/context; no real owner session. |
| [04 Answer 20](screens/04-answer-send-missing-20.png) | Send is visible. Filename reflects the retracted diagnosis. |
| [05 PM fixture](screens/05-pm-fixture-18.png) | Scripted text; missing source timestamps remain unknown. |
| [06 Summary unknown](screens/06-summary-unknown.png) | Native raster, not an OS-slot capture. |
| [07 Summary pause](screens/07-summary-pause.png) | Native pause raster, not actual shell placement. |
| [08 Home after](screens/08-home-bell-after-16.png) | Current F2 after; count and warning retained, guard passes. |
| [09 Settings after](screens/09-settings-bell-after-20.png) | Current F2 after, light/20 px; guard passes. |
| [10 History beginning](screens/10-native-history-start.png) | Native first records, 1470×890, light/20 px; retained draft and truthful missing-save state. |

[Question clip](native-question.mp4): 19 sampled frames, 9.5 s playback. [History clip](native-history.mp4): 17 frames, 8.5 s playback. Both are encoded at 2 fps from the instrument's WKWebView snapshots; manual measurements add frames, so playback length is not wall-clock interaction time. No OS chrome/cursor/audio or physical-device proof. Three evenly spaced decoded frames per clip were visually inspected; no stale prototype or missing target was substituted.

## Canonical gate

`python3 scripts/check.py --base cbeb8db913aa0908fbe9490e7de8404e7a306231` ran once on frozen evidence candidate `311c6b76df4c2699e0a111c1a1ddb02322464a77`. Selection was full. Outcome: **sandbox-blocked, not PASS**, command exit 1 after 241.006 / 900 seconds, no retry.

- Rule/change checks and web bundle generation passed; no changed contract/rule assertion was proposed.
- Rust unit driver: 752 passed, 0 failed, 35 ignored, 63.35 s. This includes the actual 10 MiB/100 MiB/oversized-source and 10,000-record transcript checks; their source-reader boundary remains distinct from native UI/cold-warm I/O.
- Integration: 21 passed, one blocked, one ignored, 4.26 s. `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, `tests/telemetry_process.rs:189`, reports `sandbox-exec: sandbox_apply: Operation not permitted`.
- The unit log separately records `seatbelt_keeps_workers_out_of_the_policy_store` sandbox-blocked. No permission, containment or assertion was weakened.
- Later offline selftest and canonical component/artifact stages were not reached. Independent browser/native/component outcomes above keep their own attribution; they do not convert the full gate into PASS.
- The final evidence-only note changes no application/test/dependency/instrument or native capture. The full gate is not repeated for that note (§7.4, R12). Merge-result `merge_group` CI remains pending; no CI wait, merge or rollout is claimed.
