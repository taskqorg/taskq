# Conversations recovery and working document

Implements [#2140](https://github.com/taskqorg/taskq/issues/2140) on baseline `fbc17b53acd043510ce0da4cad32433606ed5631`. The task preserves the owner's request verbatim. Applied R13 before implementation: Product, P-W3, R-UI3, TW2/TW5 and C2/C7/D4 permit dependable Conversations recovery chrome and retain the accepted single centre. R1/R9 preserve board authority; R12 separates source/browser evidence from installed acceptance; §7 governs publication.

## Before and causes

The independent installed Mac before packet has SHA-256 `7b0853cf38d0a0d5b0c50ae2684a9f9168bc8dc193d161d34545876685153957`. N01 records Work sessions / In progress → coreq #112 → whole-workspace Interface error; Escape did not recover. N02 records repeated workspace-save refusals. That episode's exact source error and disk condition were not captured; neither an unattributed log line nor the following reproductions establishes its original cause.

Three isolated failures establish repairable causes:

- An accepted transcript reply with no item array throws in `workerSettle`; the early guard hides the workspace. The actual-bundle regression fails before with “an invalid accepted local-source page must not replace the whole workspace”. The shared transcript check now reports source unknown inside the reader, retaining the same board/task/root/attempt and retrying reads only.
- A 32-entry Back stack with large disclosure sets exceeds the native 65,536-byte limit. `tray-view-state.test.cjs` fails before with “workspace Back state exceeds native 64 KiB”. Oldest Back entries now leave the serialized window until it fits; current source, focus and scroll remain exact. A current entry that cannot fit refuses explicitly. Native validation/write/readback failures preserve prior navigation and name their class.
- The old independent fault notice has no recovery controls. `tray-web.test.cjs` fails before with an empty action list. Reload safely, Back to list and Close now call only the native recovery bridge, even if the product API has failed. Native framing and the native application/tray menu also work outside that document's renderer.

The workspace also lacked forwarding for its existing `completed` reader operation. It now reaches the existing registered-root adapter rather than the unsupported-action fence. This is a source defect, not proof of the original N01 cause.

## Candidate and preservation

Native reload recreates only the targeted Conversations WebView in its existing window. A generation stamp from the native closure fences retired IPC, read replies and evaluation callbacks. Main and detached readers retain their identities; reopening a faulted reader recreates it, while healthy Close hides it. Native title/drag/close/enlarge replace duplicate custom framing; navigation, Always on top, resize and task features remain.

The native process retains a bounded recovery checkpoint across reader recreation: dirty local draft text/files/caret, inert local-only queued storage work, original pending operation identity, exact selection, reading anchors and focus. Recovery does not dispatch any conversation action, including local storage changes. The interrupted operation remains unknown under its original ID until explicit reconciliation/retry. Sends, answers, watches, admissions and report decisions are excluded from replay. Existing native stores retain their immutable receipts; unknown operations require explicit same-ID Check delivery. Source changes keep recovered text/files explicit and block Send. Ordinary saves still own durable restart recovery; failed storage is never claimed durable. Total recovery checkpoints are limited to 32 MiB; overflow is explicit and refuses reload rather than discarding current edits.

Worker text uses the existing bounded transcript renderer as a document, without an outer reader disclosure. Recorded questions retain their source card, evidence, read-only numbered choices and answer order, with consistent author/time spacing. Missing authors/times remain explicit. Historical question content no longer floats a linked task heading into its history. Current questions/actions stay primary; one Earlier history disclosure retains old events/questions and grouped tools retain their inspectors. No adapters, counts, icons, semantic colors, compact-time rules or source paging budgets change.

## Verification

| Acceptance | Evidence and boundary |
|---|---|
| A1 | Native before inspected; isolated source/fallback/save failures reproduced and traced above. Original installed episode attribution remains unknown. Specs amended before implementation. |
| A2 | Actual bundle opens revision-lazy local Codex/Claude workers with exact root/board/task/attempt, validates malformed accepted pages, and preserves explicit refusal. Rust save/reload storage and generation test; existing worker native-source refusal tests retained. |
| A3 | Three reader faults per matrix cell; independent fallback Close/Reload without product JS; native menu/framing/recreation compiled and source-reviewed. Actual OS click/raise/repeated recovery is Not tested. |
| A4 | Dirty text, owned file reference, caret and focus restored across destroyed browser realms. Pending same-ID steer remains unknown without automatic answer/send/report/read actions. Existing full-flow unknown-receipt tests retain explicit same-input checks and paragraph anchors. |
| A5–A6 | Existing source-reusing worker and historical question/answer rendering; worker tool folding, lazy source bounds, Latest follow, scroll/selection and advanced inspectors retained. Current reader captures visually inspected. |
| A7 | Chromium: Codex/Claude × dark/light × 18/20 px × 380/900 px, 16 cells, three faults each. Existing worker, question, attention, time and flow checks retained. Browser proof does not qualify installed clients. |
| A8 | Exact-source Codex review and selected canonical gate recorded below. Merge-result CI is a publication gate, not a worker wait. Installed Mac/Windows acceptance remains separate. |

The worker scroll instrument initially timed out waiting for Latest after programmatic `scrollIntoViewIfNeeded`. The existing protection correctly ignores layout-generated scroll. The fixture now dispatches its explicit instrument intent; it keeps the same append/anchor/Latest/attempt assertions. Real wheel intent remains covered by the full-flow instrument. The historical answer assertion changes only presentation matching: author and exact text still follow their original question, all choices stay read-only, and the next open card stays last.

## Limits retained

Earlier #2110 measurements remain failures: 10k interaction p95 134.5 ms against 100 ms; frame p95 Claude/Codex 83/67 ms against 32 ms, with static/blank controls 66.7 ms. The additional Claude delay and longer engine-heap plateau remain unqualified. No threshold is lowered. Source/browser success does not accept the installed Mac or Windows product. No live PM message, task answer, report decision, real Held admission, production fault injection, installation, restart or host policy change was performed.

## Review set

- [Original installed Mac failure](before-native.png): independent before evidence, not a candidate pass.
- [Worker document](worker-document.png): browser with native-frame content mode, same source and explicit unknown receipt; no OS titlebar is simulated.
- [Recovered local draft](recovered-draft.png): retained file, text and caret after repeated reader recreation.
- [Recorded question document](question-history.png): existing question renderer, author/time and read-only choices beneath the current task context.

Latest mixed-fixture run retained functional bounds but failed the 100 ms interaction budget: 600/10k p95 382.4/307.2 ms; projection 178.2/148.9 ms, mount 439/1707 ms. It overlapped browser recovery and native compilation on this Mac; load was present, but causality is not established. These are failures, not a qualified performance pass. Fifty-tour engine plateau remains unqualified.

## Exact-source review correction

Independent Codex review of `cc88367e2c4ee3549dc4095413a69d1e3249405f` found P1: recovery omitted `pending` and immediately submitted recovered local save/attach/remove work with new IDs. The strengthened actual-bundle check fails before because the checkpoint omits the original pending save ID. The fix checkpoints the immutable pending request before submission, restores it as unknown, keeps recovered local work inert, and forbids every automatic `conversation-action` during recreation. The same 16-cell/three-fault matrix passes after. Existing same-ID delivery checks remain explicit.

The first canonical run selected full checks after the one rebase. It stopped sandbox-blocked: 832 Rust units passed, 7 protected-app-server cases failed with `PermissionDenied: Operation not permitted`, 40 ignored; 490.195 s / 900 s. Those Rust sources are unchanged by the review's JS-only fix. No sandbox-blocked test is a PASS. The unrun remainder is checked separately on the final source; the whole gate remains sandbox-blocked. The pinned contract lint was unavailable in the trimmed local rule cache; the same lint from verified specq `39e6024037fb69e68a84180c04fb7ac53eddb9f9` was fetched read-only and passes separately: 54,486 bytes, 274 lines, 0 errors, headroom warnings retained.
