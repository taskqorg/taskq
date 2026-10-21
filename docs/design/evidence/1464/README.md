# Workspace requests and task answers · #1464

This candidate moves owner request composition and task answers from the tray menu into the workspace (R6/R8/R12/R13, § 7; TW1/TW2/TW8). The owner answered the scope question with option 1: full replies stay native; attachment handoff is deferred to a separately approved task. No conversation store, transcript or proposal protocol returns.

## Feature and preservation ledger

| Requirement | Delivery and evidence |
|---|---|
| A1 / TW2 | Menu request and task-answer buttons open the exact workspace route, project, board, task and launch revision without sending. The adapter test also supplies two roots for the same board; selected-project capability and operation routing do not substitute the other root. The workspace native bridge owns request/answer effects. Operational review paths remain in the tray for #1465. |
| A2 / V15/V28 | The existing public answer card and canonical CLI dispatcher retain original choice IDs/order/text, recommendation, literal free text, attempt/revision display, ordinary direct choices and confirmed fallback Close. No independent parser or new board client. |
| A3 / disabled Send | The historical owner symptom cannot be re-observed against a live owner draft here. The retained #1430/#1456 browser sequence types through changed refreshes and checks the same textarea, valid Send and visible limit errors. The new request sequence covers empty/valid input and Send/Enter agreement. |
| A4 / approved scope | Request and answer drafts/caret use native persistence; Back, changed snapshots and page reload preserve them. Enter uses Send validation; Shift+Enter and IME do not send. Full replies remain native and attachments are explicitly deferred. A pending request keeps its original project even when a different project launcher is opened. Physical/native IME is not qualified by synthetic browser events. |
| A5 / V28 | Existing foreign/late receipt, duplicate, exact revision, same-ID retry and partial fallback tests run against the moved adapter. Native setup records bind one request ID to immutable input; restart and Check delivery read that ID, including after launch. Check delivery is read-only: a missing/deleted/corrupt record cannot create a request. Pending persistence restores as unknown and never automatically replays. A failed preference write after request creation leaves unknown, never authorizes a new-ID retry. |
| A6 / G08 | Before: `node tests/workspace-communication.browser.cjs` failed `0 !== 1`: the original workspace had no named Refresh recovery control. After: focused Refresh and focused text/caret survive changed updates, unavailable routes have a named Messages fallback, and routine status uses a polite live region. The original #1443 BODY-focus observation is historical; the #1463 shell had already replaced that renderer. |
| A7 / V27/V36 | Loading, missing project/runtime, stale/partial project snapshots, failed saves, unknown request/answer delivery and disappeared tasks have explicit recovery. Corrupt preference bytes remain unchanged and do not become an empty writable store. Refresh performs no mutation replay. |
| A8 / V38 | Five distinct-gap browser images below, plus the two README-native captures under `docs/shots/` (seven images total). Actual-size owner review is pending. Browser source/interaction evidence, native renderer qualification and merge-result CI remain separate (R12). |

Additional failure-before/pass-after: the request test observed retained textarea text after an accepted request. The shell adapter now passes textarea `value` to Preact; the same test verifies clearing only after its exact receipt, with active input/caret retained on ordinary updates. The restart sequence also caught delayed disclosure persistence hiding a saved answer; saving the card now flushes its view state before navigation/reload.

## Checks and limits

The 17 Node component/IPC checks pass. The offline selftest passes 97 fold traces, format checks and staged private telemetry capture; board access was intentionally offline. The five setup tests pass, including immutable ID/whitespace binding, no-start/unknown separation and missing-record read-only recovery. No test rule was weakened. Provenance hashes were refreshed for the changed shared card; existing finding and card conformance fixtures still run.


- `node tests/workspace-communication.browser.cjs`: passes Refresh/input focus, polite status, Send/Enter, Shift+Enter/IME, immutable retry, request and answer reload/caret, no replay and unavailable/stale recovery.
- `node tests/answer-card.browser.cjs`: the unchanged public mount/capability/send contract also passes standalone light/dark, 16/20 px and 280/480 px keyboard/refresh instruments. Optional native load/save does not grant authority to other consumers.
- `node tests/tray-answer.browser.cjs`: the existing card adapter sequence passes in light/dark at 16/20 px, now in the workspace. It retains literal free text, changed questions, sending refresh, same-op readback, Check delivery, late receipts, fallback confirmation and partial outcomes.
- `node tests/tray-answer.test.cjs`: moved adapter retains capability, foreign receipt, duplicate, timeout, readback, partial fallback and shared fixture protection.
- `node tests/tray-home-ai.test.cjs`: menu launchers retain exact scope/revision and contain no composer or answer card.
- `node tests/tray-view-state.browser.cjs` and `node tests/shell-navigation.browser.cjs`: navigation, independent window state, Back/invoker focus, changed snapshots and existing target checks pass. Project selection now belongs to the workspace rather than a removed menu composer.
- `cargo +stable test --locked --bin taskq workspace_local_preserves -- --nocapture`: passes native draft/caret/op persistence, corrupt/oversized input preservation and absence of replay.
- Selected gate `python3 scripts/check.py --base origin/main`: incomplete in the worker environment. Its Rust phase reports 444 passed, 18 failed, 34 ignored in 47.028 s total on the final native source. Host capacity/lifecycle/installation tests refuse inherited worker authority (for example `a worker cannot restart the host service or request Stop`). No authority variable was removed to bypass these refusals. Host tests are sandbox-blocked, not PASS; publication still requires green `merge_group` CI (§ 7/R12).

The native backend remains the existing qualified Claude owner-session path. Codex owner-session launch is unavailable here and is explicitly named; no runtime fallback or permission change was added. The any-runtime TW4 target remains unqualified. Messages here lists current questions only; message badge/archive/delete belongs to #1467.

No live board answer, paid/native agent launch, service change or installation was performed by these fixtures. They substitute native delivery while testing the actual bundles. Exact binary/native renderer qualification and owner review remain open.

## Actual-size review set

Chromium headless shell via `tests/browser.cjs`, actual bundled product HTML. Invented data; no real board effects. Each image is at its CSS size: workspace 800×600, dark 16 px, except the narrow fallback at 360×900, light 20 px. Source SHA-256 and image hashes are in `sources.json`.

1. [Request ready](request-ready-dark-16.png): named project/runtime, valid draft and Send.
2. [Delivery unknown](request-unknown-dark-16.png): kept draft, explicit same-ID recovery.
3. [Unavailable task](task-unavailable-dark-16.png): named recovery without routing to another task.
4. [Restored answer](answer-restored-dark-16.png): source choices/order and persisted draft/caret.
5. [Narrow fallback review](fallback-close-light-20-360.png): original consequential confirmation and receipt, with prior accepted answer kept separately.

The builder inspected the rendered images individually. The first request image exposed the browser's default inline/monospace textarea; the workspace now inherits product type and spacing and keeps a compact Send target. These images are browser evidence, not WKWebView/WebView2 or owner acceptance.

The two native README captures are [project menu](../../../shots/popover-dark-project-16.png) and [summary icon](../../../shots/icon-summary-run-dark.png). Their exact binary, source commit and UI blobs are recorded in [the native manifest](../../../shots/manifest.json). They do not qualify the workspace native bridge.
