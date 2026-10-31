# Atomic menu refresh investigation

The owner's report is [task #2028](https://github.com/taskqorg/taskq/issues/2028). Its original live symptom was not reproduced. The change removes a confirmed intermediate-publication path; synthetic replay establishes its failure before and pass after. It does not establish that this path caused the owner's observation (R12, §7.4).

## Publication boundary

M3.26 was written before implementation. A native refresh now delivers its snapshot and dependent callbacks in one `updates` packet. The controller adopts only the final snapshot, then its callbacks, before one DOM render, native fit and navigation save. An intermediate missing project cannot overwrite the saved destination. A deferred release uses the latest adopted snapshot. A held pointer still defers publication, and an interrupted packet does not publish its intermediate render.

| Suspect | Finding and protection |
|---|---|
| Partial/stale snapshot | `src/tray-app.js:710` batches adoption and dependent renders. Synthetic partial/error and stale-line inputs followed by their complete result must never escape the packet. A settled unknown still renders its reason (R12). Separate real board observations remain separate updates. |
| Project reads arriving separately | `src/tray.rs:2263` already collects every project before pending projection and the display model. It does not stream projects into the menu. The replay protects the publication boundary if a packet contains intermediate project sets. |
| Pending overlay | `src/tray.rs:1545` previously delivered the receipt immediately, then queued `Sync → Tick` to publish its readback. That allowed a receipt-triggered render on the previous snapshot. It now sends the shared pending readback and receipt together to the menu. Native pending policy and workspace receipt handling are preserved. |
| Deferred presentation | The deferred function already reads the latest state. Replay keeps that protection and checks that packet publication cannot replace a pressed target. |
| Initial native callbacks | `src/tray.rs:1854` previously sent the restored snapshot and native-window flags separately. They now settle before the first fit; restored All, Completed and Running views each emit one fitted view. |

No timer hides genuine errors or imposes a minimum lifetime on real board states. The change removes intermediate output within a refresh; it does not smooth separate board observations.

## Failure before, pass after

`node tests/tray-refresh.test.cjs` replays synthetic sequences authorized by the task's PM answer. It uses the actual controller and vnode output, renderer timestamps, no user-input events, and a 2,000 ms disappearance check. The pre-change delivery path fails with `partial snapshot: row vanished without input after 1000 ms`. The changed packet path passes. The same test retains settled unknown reasons, press/release, latest-generation and interrupted-packet protection.

`scripts/check.py` discovers every `tests/*.test.cjs`, so the replay belongs to the canonical gate. Its browser companion exercises the actual bundle/Preact and native-size IPC boundary across restored All, Completed and Running views. Each packet sends one size measurement and retains persistent unknown state. Those three final browser frames were visually inspected: headers, controls, counts and task content remained visible without clipping. This is static regression inspection, not native motion acceptance.

Retained checks: `tray-press.test.cjs`, `tray-view-state.test.cjs`, `tray-view-state.browser.cjs`, `partial-project.browser.cjs`, `pending::tests`, and the native web-callback stall/ack regression. Existing decided assertions were not weakened or removed.

## Mac observations

The original read-only All tasks trace lasted 907.631 s with 433 full snapshots of 11 registered projects and no transient task rows. Its narrow selector and absence of installed pending callbacks made it insufficient for the owner's report.

The wider baseline uses the real WKWebView renderer and saved navigation, every menu element's direct text, attributes, visibility and geometry, section open/closed state, menu dimensions, full input packets, native answer callbacks and input-event markers. It covers 11 registered projects. Its final observation contains 434 inputs, 125 changed frames and 859 visible heartbeats over 909.155 s; the largest heartbeat gap is 1.350 s. No user input or element removed within 2 s was recorded. Initial frames were also examined for removals: none. This is a bounded observation, not proof that the owner's symptom is absent.

The post-change observation and its exact duration/counts are reported in the task result. One earlier post-change run was invalidated by user input and window hiding; it is not a pass. The replacement dev observer preserves navigation, stays pinned without taking focus or staying above other apps, and disables notification dispatch. It produced input packets but no paint frames or heartbeats and was stopped; it does not count as A6.

The installed tray could not be instrumented in the worker: removing its exact exited process's heartbeat returned `Operation not permitted`. Its normal installed executable was subsequently observed running again. Installed pending-store and callback qualification is **sandbox-blocked**, not PASS. The dev observer has its own pending store; no owner operation was fabricated to populate it.

`tray.log` has no timestamps, so its lines cannot be attributed to 05:20–05:30Z. The local telemetry export contains committed observations of receive/render/resize activity in that interval, including repeated renders at 05:28:58Z, but no DOM contents; it does not identify the flickering element.

Raw journals, exact diagnostic sources/binaries, exports and hashes remain in ignored `.taskq/2028/` because they contain live project data. They are not public artifacts. The temporary recording branch is excluded from product code. The task result reports the canonical gate and remaining A1/A2/A6 qualification gaps separately from the deterministic proof.
