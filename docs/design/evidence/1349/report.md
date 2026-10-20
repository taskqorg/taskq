# Running agents menu block

Candidate implements #1349 and owner answer 1 under R6/M3. Home shows observed running agents by host/runtime; expansion separates workers and reviewers. Reservations and occupied slots stay separate from task-state totals.

## Evidence

- Before: `node tests/tray-running.test.cjs` failed because the running-agent renderer did not exist. After: passes host/runtime/role counts, stale and incomplete observations, reservations, duplicate boards and escaped names.
- The existing execution-heartbeat Rust check now also requires role/runtime to survive the local observation overlay and host rows to reach the status file. All fencing, stale, malformed, trust and terminal-state assertions remain.
- The status golden fixture adds role/runtime metadata; no existing expected value or assertion is weakened.
- `node docs/design/evidence/1349/capture.cjs` renders the real bundled tray with invented data through the safe headless browser launcher. No native bridge or board effects run. See `observations.json` for renderer and text readback.
- Compact dark and expanded light images were visually inspected, together with the full dark Home framing. Runtime totals, role splits, stale age and the reconciliation text are visible without clipping in the captured block. Full Home uses its existing scroll area.

## Owner review

Approve the compact host rows and their expanded detail in both themes before merge (A4). Browser fixtures do not establish installed WKWebView/WebView2 or live-board acceptance. No admission, permission or lifecycle policy changes.

![Dark compact](dark-compact.png)
![Light compact](light-compact.png)
![Dark expanded](dark-expanded.png)
![Light expanded](light-expanded.png)
