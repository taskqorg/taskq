# Notifications panel · #1964

## Decision and baseline

Amended rule: **R13**; amended product specification: **M14**. R6/R13: move tray results and events to the bell-opened M14 panel. Keep current work and unanswered questions on Home. Preserve SHA-bound result reads and every existing result reaction; clear notification metadata only. R1: the board remains task/decision truth. R12: browser, native renderer and Windows checks are separate claims.

Baseline `0426dee15143f745bfd0df313f183750ab25c386`: `home()` renders `reports.attention()` before the task dashboard and `reports.section()` afterwards. `head()` adds results to Conversations and can render `1+?`; `notifications` is OS settings. Ten results therefore precede current work. These are the reproduction targets.

## Alternatives on the same fixture

Invented fixture: ten Cadence v4 results, one unresolved question, current work and one failed-attempt event. Selection follows [comparison rules](../../comparative-workflow-review.md#fit-to-taskq-principles): owner need first, board state canonical, no notification age-out as resolution, reuse existing cards and transport.

| Standard pattern | Benefit for this fixture | Cost / boundary | Pick |
|---|---|---|---|
| [OS notification centre](https://support.apple.com/en-au/guide/mac-help/mchl2fb1258f/mac): stacked notifications and clear | One expandable stack keeps ten results compact; familiar bell destination | OS dismissal and delivery permissions cannot define task decisions or reliable unread state | Adapt grouping; retain current OS delivery separately |
| [GitHub inbox](https://docs.github.com/en/subscriptions-and-notifications/how-tos/viewing-and-triaging-notifications/managing-notifications-from-your-inbox): read state, grouping, bulk triage | Clear unread count; explicit bulk read and remove-read flow; no result rows displacing work | Another destination to open; GitHub Done/unsubscribe semantics must not close TaskQ tasks | **Pick** one compact event inbox behind a bell, with series grouping |
| [cmux Feed](https://github.com/manaflow-ai/cmux/blob/main/docs/feed.md): unified agent decisions and exact-target routing | One event opens the relevant question/result context | Feed's runtime-specific approval integrations are unnecessary; notification acknowledgement must never answer | Reuse exact-card routing, keep TaskQ question authority |

Sources inspected via official documentation; competitor applications were not exercised. No competitor architecture or runtime permission is imported.

## Owner walkthrough and verification

The fixture is `tests/fixtures/notifications-shots.js`; the app uses the bundled bell and shared shell/cards. It intercepts every board/task write. No real task, result decision, owner session, OS permission or service change is performed.

| Owner task | Walkthrough and observed result | Evidence |
|---|---|---|
| Find what needs me | Home has two current tasks, including the question. Reading/clearing its notification leaves that question and its exact answer destination intact | M-A24, M-A26; Home native PNGs; `tests/notifications.browser.cjs` |
| Check a finished result | Bell shows 12 unread events; ten Cadence results occupy one expandable row. Its newest member opens result #109 with Changed/Checks/Open and existing reactions | M-A24–M-A25; panel/result native PNGs |
| Clear noise | Mark all read uses the displayed identities; a later failed event stays unread. Clear read removes confirmed read rows, leaving that event and the unanswered question on Home | M-A25; cleared browser PNGs; bulk/read-state tests |

The original macOS review set contains eight screens: six WKWebView captures (Home, grouped panel, result in light/dark) plus two browser clear-state captures. The current Windows continuation has its own eight-screen review set below; it supersedes that review request rather than adding screens to it. Native captures are 320 logical px wide, 16 px text, 2× raster. Chromium checks both themes at 320 px and 16/20 px; the fixture fits without horizontal scroll. Exact candidate commit is the PR head; generated binaries and source-hash manifests are not committed.

Visual review compared the affected views with the shared shell/reference rules in `visual-style.md`: intentional changes are the bell, results moved out of Home, the compact event list/group and bulk controls (A3–A5). Unaffected shell, typography, native/task destinations and result reactions are retained. An initial reader button broke a word at 320 px; the final label is `All events`. Each native scene/theme uses an isolated invented result identity, so a prior capture cannot retain its selected/read card. The fixture resets partial-count metadata as well as task rows.

## Before and after

- Before: the new Home-isolation check failed with **11 result elements instead of 0** on the ten-result fixture. After: the same check passes; Home contains only current tasks, with an independently visible question.
- Before: moving the reader hid the Make tasks success message after the report disappeared. After: the existing report-action matrix passes, including held-task receipts and failed decisions.
- R8 contract-size guard initially failed: 61,631 bytes exceeded 60 KiB. R13 wording was shortened without changing its obligations; the unchanged guard now passes at 61,436 bytes and 300 lines.
- Native command queue capacity is 16. Bulk read now submits one result read per receipt; a 30-result regression passes without overflowing it, repeated clicks are deduplicated and a later arrival stays unread.
- Source provenance initially rejected the new module because its manifest entry was not sorted. The existing `--write` source-list generator corrected the order; the unchanged provenance/hash check passes. Only file names are committed, never generated source hashes.
- Read/clear state keeps exact event/result identities and survives serialization and actual tray-settings disk save/reload. OS delivery cursors and settings remain unchanged.

## Verification status

| Boundary | Status | Evidence |
|---|---|---|
| M-A24–M-A26 browser behavior/geometry | Pass | `node tests/notifications.browser.cjs`: four cells, dark/light × 16/20 px, 320 px; failures, late arrival, 99+, partial history, exact targets and question retention |
| Identity/grouping/read model | Pass | `node tests/notification-panel.test.cjs`; duplicate registration/close suppression; different projects, kinds, series, repeated numbers, interleaved events and ten-minute boundaries |
| Bulk read / failed and unknown receipts | Pass | `node tests/reports-bulk.test.cjs`: 30 results; one native submission at a time |
| Native metadata | Pass | `cargo test --locked --bin taskq notify::tests::inbox_markers`; `cargo test --locked --bin taskq tray::tests::pin_and_position_per_display_survive_restart` |
| Existing reader and navigation protection | Pass | Reports and Results browser matrices, header geometry and tray navigation checks; only entry-route/count expectations changed, all decision/error/draft checks retained |
| macOS WKWebView build/capture | Pass | Opt-in `notifications-home,notifications-panel,notifications-result` snapshot routes; six PNGs; final images visually inspected |
| Windows WebView2/tray build and native capture | Pass | Exact implementation commit `98391e68562c748d89547e55f442da3173c76c61`; native Home/panel/result captures in both themes, plus four native WebView2 behavior cells; see the Windows continuation below |
| Canonical selected gate | Sandbox-blocked | `python3 scripts/check.py`: 737 unit tests pass, 35 ignored; 21 integration tests pass, one fails at `sandbox-exec: sandbox_apply: Operation not permitted`, one ignored. 147.399 s. This is not a green gate (R12/§7) |
| Remaining canonical phases | Pass | Offline selftest: 113 traces, formats, committed capture; board/tools explicitly unknown. All Node unit and artifact-script checks pass when run separately after the blocked Rust phase; affected browser matrices pass |

An additional unchanged `tests/tray-panels.browser.cjs` run reached its obsolete `Open codex session` locator and timed out. UI30's current block exposes the native destination in details as `Open native session`; this task does not change that unrelated destination or its guarding assertion. The focused notification checks verify question/result targets separately. This observation is not counted as a pass.

Windows-native panel evidence is complete. Owner visual approval and merge-result CI remain open under R12. The Windows GNU canonical gate has an unchanged sandbox-preparation failure, detailed below; it is not a pass. No deploy or live-host installation is claimed.


## Original macOS review images

| Scene | Dark | Light |
|---|---|---|
| Current work and question on Home | [PNG](native/popover-dark-notifications-home-16.png) | [PNG](native/popover-light-notifications-home-16.png) |
| Grouped Notifications panel | [PNG](native/popover-dark-notifications-panel-16.png) | [PNG](native/popover-light-notifications-panel-16.png) |
| Exact completed result | [PNG](native/popover-dark-notifications-result-16.png) | [PNG](native/popover-light-notifications-result-16.png) |
| Clear read preserves a later unread event | [PNG](browser/cleared-dark.png) | [PNG](browser/cleared-light.png) |


## Windows continuation: A7

The reviewer accepted A1–A6 at `98391e68562c748d89547e55f442da3173c76c61` and returned A7 because Windows was Not tested. This continuation changes evidence only. It does not change the accepted implementation, specifications or guarding assertions (R13/§7). The original macOS evidence therefore remains applicable to the same runtime source.

- **Build:** `cargo build --locked`, Pass, 304 seconds, debug, `stable-x86_64-pc-windows-gnu`; Rust `1.99.0 (b940084d7 2026-09-28)`, Node `24.19.0`. The existing WebView2 loader is staged beside the executable. Binary SHA-256: `fb883f87a3012fde621815e4a4af0cee5124a6c3a768676b92e52aba3eafe917`.
- **Target:** Windows `10.0.26300.0`, native WebView2 `156.0.4314.50`, 320 CSS px wide, observed DPR `1.399999976158142`. Six images use the product's `tray --snapshot --route notifications-home,notifications-panel,notifications-result` adapter. Two clear-state images use CDP against that same executable's WebView2, not a standalone browser.
- **Behavior:** the existing `tests/notifications.browser.cjs` assertions ran through a temporary WebView2 transport adapter, using [Microsoft's documented CDP connection](https://playwright.dev/docs/webview2). It attaches to the existing native page, preserves the bundled HTML, ignores empty native fixture ticks, supplies the existing theme palette and uses separate invented SHA identities for four cells in one window. Every original behavior assertion remains; the fixture intercepts all task/result writes.
- **M-A24–M-A26:** Pass in four native WebView2 cells, dark/light × 16/20 px, 320 px. Home isolation, distinct question entry, newest-first grouping/expansion, exact result/question/task destinations, read/clear, failed receipts, late arrival, retained question, 99+ and incomplete-history text all pass. Both document and content overflow assertions pass. [Native transcript](windows/webview2-check.log).
- **Isolation:** a transient interactive scheduled task launches only the owned candidate and temporary fixture installations/profiles. The ordinary host service, production tray, board, grants and settings are not changed. The candidate is stopped and the temporary scheduled task removed after the check.

### Canonical gate and limits

`python scripts/check.py --base HEAD` on the exact Windows implementation checkout selected the full suite. It **failed** after 268.016 seconds: 730 unit tests passed, 39 were ignored, and `windows_sandbox::preparation::tests::native_preparation_and_restore_preserve_controls_and_data` failed at `src/windows_sandbox/preparation.rs:259`. The test expects deleting its fixture WAL file to return OS error 5; deletion returned `Ok(())`. Integration and later gate phases were not reached. That file is unchanged from the task's base `ed179014e7b7fd4ad1f08c822142924dc8ea9382`; no assertion or security rule was weakened. This is a failed GNU-host gate, not sandbox-blocked and not a green Windows gate.

The installed MSVC Rust toolchain has no `link.exe`; a disposable compiler smoke check reports `linker link.exe not found`. No Visual Studio installation or global toolchain change was made. The macOS resume integration failure was reproduced with the filtered test: `sandbox-exec: sandbox_apply: Operation not permitted`; it remains sandbox-blocked, not Pass. The original source SHA's Linux PR CI passed. Publication still requires green merge-result `merge_group` CI under R12/§7; these observations do not replace it.

### Current review set: eight Windows PNGs

All eight final images were opened and inspected. The intentional changes remain A3–A5: work on Home, a numeric bell and one grouped notification destination. No clipped control, horizontal overflow or missing task/result target was observed. Native platform/frame and theme evidence is separate from the intercepted board transport; this is not a live-board or deployment qualification.

| Scene | Dark | Light |
|---|---|---|
| Current work and unanswered question | [PNG](windows/popover-dark-notifications-home-16.png) | [PNG](windows/popover-light-notifications-home-16.png) |
| One grouped Notifications panel | [PNG](windows/popover-dark-notifications-panel-16.png) | [PNG](windows/popover-light-notifications-panel-16.png) |
| Exact result after opening the group | [PNG](windows/popover-dark-notifications-result-16.png) | [PNG](windows/popover-light-notifications-result-16.png) |
| Clear read retains the later unread event | [PNG](windows/cleared-dark.png) | [PNG](windows/cleared-light.png) |
