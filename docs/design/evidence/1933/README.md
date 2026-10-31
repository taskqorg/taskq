# Native Windows qualification checkpoint

Historical first-attempt checkpoint. #1994 is now integrated; the [resumed checkpoint](resume.md) retains this failure and records the same native command passing. Broader qualification remains incomplete.

**Incomplete: blocked by [#1994](https://github.com/taskqorg/taskq/issues/1994).** This is preserved attempt evidence for #1933, not native acceptance or a final report. R12 keeps every unverified claim unknown. The evidence-only scope routes implementation changes to a separate task; no product source, assertion or contract was changed (R13).

## Candidate and provenance

- Source and fetched `origin/main`: `554552daf48d5b413918d54a8a6d0f88efba4020`, TaskQ 4.6.49. The assigned `taskq-1933` branch was clean when built; its initial rebase reported up to date.
- `npm ci --ignore-scripts --no-audit --no-fund`: exit 0, 34 packages. `cargo build --locked`: exit 0, 6m 08s, 23 warnings. No source changes or dependency updates.
- GNU development executable: `target/debug/taskq.exe`; SHA-256 `6112ec8f3c230d963150608764e85ebde98e67aac8d22d36ecc2d9f57b54d8f5`. This is a local debug build, not a staged release or installed product qualification.
- Rust/Cargo 1.99.0, `stable-x86_64-pc-windows-gnu`; Node v24.5.0. Cargo inherited `CARGO_BUILD_JOBS=5`.
- Windows registry: display version 26H2, build 26300.9711, ProductName `Windows 10 Pro` (reported verbatim, not an OS marketing-name inference). Existing WebView2 process executable version: 156.0.4314.50; this is not a separate readback of the capture process's browser version.
- One observed screen: 2560×1440; working area `(0,0,2560,1392)`. Accessibility `TextScaleFactor=140`; system/monitor DPI and computed renderer font were not independently measured. Do not conflate text enlargement with display DPI. Native captions and glyph rendering are visible in the captures; Segoe ink calibration remains unqualified.
- Menu snapshots: forced light/dark, nominal 16/20 px; Conversations snapshots: dark 18/20 px and light 18 px. Other font/scale/work-area combinations remain Not tested. No Chromium platform flag was used.
- Review ZIP: 11,108,468 bytes, SHA-256 `e397147ee8fe6d6867af8ecfe604141e9921b0ce6c46e2f059a03851e9b3342d`. Its manifest's 194 payload entries all match size and SHA-256; the manifest itself is the 195th file. Read the README, handoff plan, acceptance and issue map; inspected the landing page and current prototype HTML. No packaged source replaced current main.

All listed merge commits passed `git merge-base --is-ancestor <merge> HEAD` with exit 0. Board close comments record green merge-result CI; those receipts establish integration, not native acceptance (R8/R12).

| Task | PR | Integrated merge |
|---|---|---|
| #1918 | #1925 | `65e80d3835d40dddaa888dc1cf7ae8b85e93db24` |
| #1926 | #1944 | `1ccf6e0526f44dcce2b9607562eef5beb65d9cf0` |
| #1927 | #1948 | `b0c5269e498824d208ddd04e4da86b3c0d45b45f` |
| #1928 | #1955 | `82fb5dc76b4a7db4dec83dea92d7b00bcb2e5ac9` |
| #1929 | #1961 | `efce57d9295abd72184778d2ab32ab4e46a683f0` |
| #1930 | #1986 | `554552daf48d5b413918d54a8a6d0f88efba4020` |
| #1931 | #1947 | `b3b5c639e94820f3d37bb27985f7bfc54166805a` |

## Executed checks and failure

The fixture is the unchanged `docs/design/tray-fixture.sh` at the source above, with three invented projects and no EXTRA projects. Alpha has ordinary, Ask, Failed and Ready rows; beta supplies a board error and a same-number task on a different forge; gamma has a deliberately old status file. The native adapter correctly presents uncertainty in the sampled screens. This old fixture does not establish complete/fresh host or exact-counter coverage.

The launch environment selected only a disposable OS-temp `TASKQ_INSTALL`, disabled incident reporting, and removed inherited task/session/token bindings in that child environment. Snapshot mode avoids ordinary tray startup and service opening. No production install, service, grant, view, owner session or task answer was changed. Native Computer Use window inventory was read only; no user input was injected.

1. Direct executable `--version` and snapshot launch failed with exit `-1073741515` (`0xC0000135`) before UI. This remains a standalone-development-layout limitation. GNU's documented Cargo runtime loader path was then used; no code or permission change was made. `cargo run --locked -- --version` passed and printed `4.6.49`.
2. The following built-in native WebView2 capture completed 50 scenes, then exited **1**, reporting `horizontal overflow: popover-dark-home-16.png button 6 by 3 px` and the equivalent bell warning on other routes/themes/fonts. Preserve this failure; do not report the capture command as PASS.

   ```text
   cargo run --locked -- tray --snapshot <menu-output> --route home,project,settings,appearance,hosts,cadence,notification-settings,agents,projects,delivery,services,recovery,about
   ```

3. Source localization: `src/tray-app.js` `measure` reports a visible button when `scrollWidth > clientWidth + 1`; its text identifies the bell's unread count `6`. `src/app-shell.css` places `.notification-warning` at `right:-3px;bottom:-3px`. The matching 3 px excess is consistent with that intentional warning. It is not proof that all native geometry is safe. #1994 must resolve the diagnostic while retaining the warning, counts, accessibility and real clipping protection.
4. `cargo run --locked -- tray --snapshot <conversation-output> --route conversations`: exit **0**, ten native renderer scenes. This snapshot path does not call the menu's geometry measurement. Its PM transcript and draft/question records are invented and intercepted by `tests/fixtures/conversation-shots.js`; it proves neither live provider replies nor a PM-to-result chain. The selected screenshots show a scrolled reading viewport, not evidence that all its content is reachable.
5. `npm run build` and `node tests/<name>.test.cjs`: **PASS** for `project-blocks`, `relative-time`, `tray-answer`, `task-workspace`, `reports`, `reply-files`, `task-controls`. These are source/component checks, not native interaction or board-effect proofs.

Raw build, version, component and native capture logs, all 60 native PNGs, the verified packet and disposable fixture remain in the attempt's OS-temp scratch area (`taskq-1933-*`). Seven unmodified representative captures and their hashes are preserved here. No native before-state capture or interaction recording exists; no before/after pass is claimed.

## Acceptance status

| Criterion | Verdict | Evidence or remaining gap |
|---|---|---|
| A1 | Not tested in full | Integration, source, build, binary hash and partial host facts pinned above; staged install, actual capture renderer version, computed font and native DPI remain unknown |
| A2 | Fail: native check; other checks Not tested | Menu snapshot guard exits 1; #1994. No real wheel/touchpad, IME, scrollbar drag, resize, font14/18 menu, DPI or complete preservation run |
| A3 | Not tested | No explicitly disposable service-owned Claude/Codex session or disposable board episode was provisioned/exercised; no live owner task answer |
| A4 | Not tested | Invented partial/unavailable presentation only; real local/remote/ended targets, files, delivery reconciliation, archive/detach/restart and exact times unqualified |
| A5 | Not tested | No native input/selection latency, frame-task trace, continuous physical scrolling, cold/warm I/O, hostile-history DOM/cache bounds or anchoring measurement |
| A6 | Not tested in full | Native Appearance/History captures only; no installed Windows slot, exact native count matrix, before-state or recordings; no macOS surface claim |
| A7 | In progress | This checkpoint, the row-by-row coverage map and selected screenshots preserve findings; final report and independent review packet remain due |

## Remaining qualification

[Coverage](coverage.md) keeps each UX30, scenario, complementary check and preservation ID separate. Its Not tested verdicts cannot be upgraded from merged tasks, mocks, screenshots or CI. Resume #1933 after #1994 closes, with the corrected integrated candidate and new provenance. Retain this failing candidate's evidence rather than overwriting it. Complete physical/native input, disposable provider/board correlation, remote/recovery, performance and supported-Summary qualification; then publish the final `docs/research/1933.md` report under §7. No release, rollout or task start follows from this checkpoint.
