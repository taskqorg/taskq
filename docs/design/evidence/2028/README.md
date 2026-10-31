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

The completed A6 observation uses the unchanged fixed product sources at `d277944047b4d16825a902434c14da6ed074eaf3` plus private recording instrumentation. From 07:30:47.085Z to 07:45:56.921Z on 2026-10-31 it recorded 909.834 s of renderer activity, including 908.838 s between visible heartbeats: 437 input packets, 153 changed frames, 854 visible heartbeats, 11 projects and zero user events. Both the text-identity scan and the independent node-visibility/removal scan found zero elements disappearing within 2 s. The last changed frame was at 896.435 s; unchanged visible heartbeats continued through 909.848 s. The native observer exited normally. [Sanitized summary](native-after-summary.json) pins the product source tree, diagnostic binary and private journal hashes. Exact diagnostic source and bundle hashes remain in the private metadata.

The observer retains the saved custom Home view and normal project reads, pending projection and native callbacks. Its separate window is pinned above other windows without focus and passes cursor events through. Task effects, notification dispatch, native settings persistence and consumption of the installed tray's raise request are disabled. It does not replace the installed tray or service. The maximum heartbeat gap was 16.649 s; the result is a bounded observation with that gap, not proof of continuous capture or every possible live sequence (R12).

Earlier post-change attempts interrupted by input/hiding, or lacking paint frames/heartbeats, are not passes. A foreground diagnostic attempt in this correction was likewise discarded after input. Its one raw text candidate was a still-visible Codex counter changing from `2` to `2+?` after a new unknown execution observation at 190.449 s; the uncertainty persisted for over 30 s. The counter node did not disappear. The final passive A6 run above had no such candidates. Product code and existing test assertions were unchanged for this evidence correction; `node tests/tray-refresh.test.cjs` passed again, and prior implementation/browser/offline checks remain applicable to the unchanged sources under the shared Testing run policy.

The installed tray could not be instrumented in the worker: removing its exact exited process's heartbeat returned `Operation not permitted`. Its normal installed executable was subsequently observed running again. Installed pending-store and callback qualification is **sandbox-blocked**, not PASS. The dev observer has its own pending store; no owner operation was fabricated to populate it.

`tray.log` has no timestamps, so its lines cannot be attributed to 05:20–05:30Z. The local telemetry export contains committed observations of receive/render/resize activity in that interval, including repeated renders at 05:28:58Z, but no DOM contents; it does not identify the flickering element.

Raw journals, exact diagnostic sources/binaries, exports and hashes remain in ignored `.taskq/2028/` because they contain live project data. They are not public artifacts. The temporary recording branch is excluded from product code. The task result separates the completed A6 observation and deterministic proof from the original-symptom, installed-pending and canonical-gate limitations.
