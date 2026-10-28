# Menu and Conversations usability review

**Preservation correction:** the initial handwritten prototype is an interaction slice, not a complete replacement. The [72-group source audit](exploration/conversations-preservation-audit.md) now maps existing functions, omissions, retained locations and regression journeys. The [current-component review](exploration/conversations-preservation/preview/index.html) preserves current menu, answer-card, conversation, View and Settings controls in an isolated local fixture. No feature removal is approved. F0 below must pass before implementation.

2026-10-28, Asia/Bangkok. Review and proposed refactoring plan for the owner's compact task menu and Conversations window. The primary outcome is comfortable daily communication with a project manager: find a question, understand it, answer, follow progress and return to earlier messages without losing context.

The menu should signal work requiring attention. Conversations should provide a stable reading area and a permanently available composer. Long history needs bounded rendering as well as bounded file reads. Delivery feedback belongs to the message or question that caused it.

Status: proposal for owner review. This document changes no active product contract or runtime. Preserve the accepted palette, bundled icons, adjustable readable typography and unrelated menu geometry. No implementation, task creation, PM message or operational action is part of this review.

## Evidence and current baseline

- Source inspected: `da4ef2b6db01b066f5c897eb5afe01269fda37df`, with pre-existing local documentation/configuration changes. The inspected runtime source files are unchanged in this checkout.
- Owner screenshot: footer `4.6.40-dev.1793116525`; a long csgo #380 question fills the menu detail; Conversations shows a partly clipped composer, extensive helper copy and an unrelated #365 answer receipt under the selected #380 thread. The owner reports scroll capture and hangs. The screenshot establishes layout, not the actual wheel route or the cause of a hang.
- Source evidence below establishes the mechanisms and gaps. It does not establish the screenshot binary's exact source revision. No installed-app interaction or timing capture was performed in this review.
- Earlier [native review](review-evidence/brief-check-2026-10-26/review.md) is historical evidence. Current code has the #1723 fixes for ordinary task entry, unified Recent/Archive selection and read-only archived transcripts; do not file these again as unchanged missing features.
- Fresh board read: [#1720](https://github.com/taskqorg/taskq/issues/1720) and [#1723](https://github.com/taskqorg/taskq/issues/1723) are closed. The [#1723 result](https://github.com/taskqorg/taskq/issues/1723#issuecomment-6418686993) and [review](https://github.com/taskqorg/taskq/issues/1723#issuecomment-6418977644) explicitly leave the installed Mac journey unobserved. Their reported browser passes are historical results, not reruns here.
- Browser reproduction was unavailable in the current root checkout: no `build/web` artifact and `require.resolve('esbuild')` fails. Dependencies were not installed for this planning review. The user's reported scroll failure and freeze remain unmeasured at runtime.

## Findings

Priorities describe the proposed work order. Source-confirmed mechanisms are distinguished from runtime hypotheses.

| ID | Priority | Evidence and consequence | Required disposition |
|---|---|---|---|
| UX01 | P1 | `task-dashboard.js:87–97` inserts the complete question, ordered choices, controls, expandable diagnostics and project answer notices into task details. `task-dashboard.css:26` makes the top-layer popover an independent `overflow-y:auto` surface with `overscroll-behavior:contain`. The gesture over it scrolls that surface; at its boundary chaining is deliberately prevented. This explains the source architecture behind the reported scroll trap; its exact native behavior is not reproduced here. | Remove long reading content from the compact menu task preview. Do not patch it by globally forwarding wheel events. |
| UX02 | P1 | `.conversation-detail` scrolls; transcript/status and composer are its children (`conversations.css`, `conversations.js:224–241`, `workspace.js:31–38`). A long transcript or question can move the input entirely outside the viewport. The screenshot exhibits the consequence. | Use a stable heading and Send tools, with naturally growing input in the sole reading viewport; short input stays at the bottom. |
| UX03 | P1 | `workspace.js:38` calls `answerNotices(p.issues)` for the selected task. `workspace-answers.js:45–51` filters by board, not selected task, so a receipt for another task on that board can appear here. Feedback also has card and overlay paths. The screenshot's #365 receipt under #380 is consistent with this source route. | Select feedback by thread/task and operation identity. One visible delivery status per action; no project-wide receipt feed in a conversation. |
| UX04 | P1 | `conversations.js:48–50,92–98` retains all loaded messages in `view.items` and maps them all into nodes. Paging bounds initial I/O, not eventual DOM size. `transcript.rs:after` can return up to 16 MiB of appended bytes. | Bound mounted history and IPC/render work; measure long-history performance. This is a plausible contributor to hangs, not a proved explanation of this incident. |
| UX05 | P2 | Prepend correction uses total scroll-height differences; selection restoration uses raw pixels (`conversations.js:49,53,289–293`). Only one transcript view is retained, and selecting another conversation later reloads its tail. Stored pixels do not identify the same message after different history windows, reflow or asynchronous growth. | Keep a stable message anchor and intra-message offset; restore the containing range. Use one authority for scroll correction. |
| UX06 | P2 | Earlier-page loading starts within 32 px of the very top (`conversations.js:281`), after the reader is already waiting at the boundary. A short first page or page with no visible records need not cause another scroll event. No new-output jump action exists in `transcript()`. | Prefetch near the upper edge, continue until the viewport is filled or history is exhausted, and expose new output without stealing the reading position. |
| UX07 | P2 | Owner replies share the workspace with launch diagnostics, watch editing, project selection, separate-request creation and long transport explanations. Task answers show a disabled Attach action and multiple instructional paragraphs (`workspace.js:94–101`). | Give reading and writing first priority; retain capabilities in context menus/details with concise, truthful limitations. |
| UX08 | P1 | `conversations-selection.browser.cjs` and `conversations-render.browser.cjs` call `scrollIntoViewIfNeeded()` before testing composer geometry. `dialogue.browser.cjs` uses 60 invented records and a positive `scrollTop` test for prepend. These checks can pass while normal wheel behavior, stable visible text, fixed input and large-history performance remain wrong. | Extend the existing instruments at the user-action boundary; do not treat reachability after forced scrolling as daily-use acceptance. |

There is no need to migrate away from Preact, replace the native WebView, add a chat server or duplicate the transcript store to address these findings. Backend/session authority and the UI's presentation problems are separate concerns.

## Contract conflicts to reconcile before implementation

1. [Visual style](visual-style.md#geometry-and-type) requires a fixed composer and scrolling transcript; [D4](ai-workspace.md#workspace-dialogue-d1d8) now puts status, transcript and composer in the same scroll. #1723 intentionally implemented the latter to remove nested scroll areas. Recommended resolution: one **reading** scroll, with a sibling composer; the composer is not a second transcript viewport. Amend D4, C3, the geometry text and affected acceptance together under R13. Do not silently change tests to a third interpretation.
2. M3 and the dashboard reference retain extensive task detail; the owner now asks for a minimal question notification. Recommended resolution: compact task identity/status plus a short attention summary and one direct Conversations action. Full question/options move to the existing thread. Preserve Pin, board access and task controls through explicit destinations; removing them entirely is not implied.
3. TW5 still says opening marks an entry read, whereas current C5 and code require explicit Mark read. Preserve explicit acknowledgement; correct stale TW5 wording during specification reconciliation. New-message tracking for navigation must not silently acknowledge events.
4. D1 forbids a transcript copy, index or summary. Proposed stable record offsets and bounded page/height data are transient rendering metadata; persisting a transcript index would require a separate decision. A saved view anchor may hold identity and offset, not message text or task state.

The owner approved this plan on #1738. Its specification reconciliation (slices 0–1) records the four resolutions in `visual-style.md`, `menu.md`, `tray-workspace.md` and `ai-workspace.md` D1/D4. Slices 2–5 are separate held tasks; native acceptance stays Not tested.

## Proposed working interface

### Compact task menu

Keep the existing header, counters, saved view and task rows. A waiting task carries its question marker and age. Opening its compact preview shows task identity, status, at most two readable lines of the source problem and **Open question**. The action raises Conversations at that exact task/question; it does not mark read, answer or launch anything. If even this preview cannot fit the usable area, keep the identity and direct action and omit the excerpt.

Do not put choice lists, a composer, answer receipts or full diagnostic text in this preview. A question preview must have no internal vertical scroll. Full original wording and source choice numbers remain available in Conversations. Other task details remain accessible through Conversations/board and the existing relevant controls; maintain a feature-to-destination ledger before moving them.

Prefer a compact inline disclosure inside the menu's one content flow for question attention. This makes wheel/trackpad gestures over the summary move the task list normally. The alternative, a small anchored preview, preserves the old position but remains an overlay hit target and needs an explicit dismissal/scroll policy. Recommend the inline disclosure for questions; this deliberately changes only the affected preview interaction and needs owner review against the frozen reference.

Close/return restores filters, selection and list position. Pin remains a task watchlist operation. A menu's Keep open and native Always on top remain distinct controls. Destructive/lifecycle controls keep their current review and readback rules.

### Conversations window

Use the accepted two-column workspace. The sidebar contains New, Search, Recent/Archive and compact thread rows; each row shows project/task or conversation identity and one attention state. Allow two title lines with a full accessible title. Preserve full identity in the selected heading. At narrow widths show list or thread with a direct Back to list action; never squeeze both into unreadable columns.

The selected pane has three sibling regions:

1. A stable compact heading: title, project, recorded runtime, current state and a menu for secondary actions. No editable title input permanently occupying the toolbar. Keep task and owner-thread types visibly distinct.
2. One reading viewport: ordered messages, question content/choices and local delivery outcomes. Use clear You/Agent attribution, timestamps when available, readable paragraphs and restrained separators. Tools collapse to concise activity lines. Technical IDs and launch/watch diagnostics disclose on demand. Do not fabricate message times or token streaming that the native reader does not supply.
3. A stable bottom composer: the current destination, editable draft, attachment chips where supported and one Send action. It remains visible while reading old messages. The heading and composer reserve layout space; neither overlays the transcript. Keep the textarea modest at rest, grow to a bounded height and allow its own native text-edit scroll only for a long draft. This is an intentional editor exception, not a nested history panel.

For an ordinary owner conversation, Send continues the recorded native session. **New from draft** moves to the secondary menu, with its separate-session effect stated before use. New still creates a separate draft explicitly. Project/runtime context remains visible; launched-session context is not disguised as a mutable destination selector. Preserve all existing supported context overrides before launch.

For a task question, show the question and exact source-numbered choices in the reading viewport. The composer says **Answer #N** and supports free text. Choice buttons submit their displayed source choice under existing revision/consequence rules; typing `1` remains literal text. A question change keeps the draft, fences all submit routes and places one concise Review updated question action beside the current question/composer. Reviewing never submits. Keep unsupported file answers clear in compact help; do not spend a permanent toolbar row on a disabled attachment picker.

Question answers remain board operations. Owner messages remain native-session turns. A task thread without a question must explain its current state and available session/board destination; it must not pretend to support arbitrary worker chat.

### Sending and recovery

| Condition | Visible behavior | Preservation/recovery |
|---|---|---|
| Ready | One enabled Send with destination clear | Enter sends; Shift+Enter adds a line; IME composition never submits. |
| Dispatch/readback pending | Submitted text has a local sending state; input does not freeze the whole window | Freeze the submitted payload; retain separately edited next text; prevent duplicate dispatch. |
| Accepted by transport | That same item says accepted/queued, as the actual receipt permits | Acceptance is not an agent reply or completion. Reconcile with the native transcript to avoid a duplicate user message. |
| Agent working | Thread state says working; new output follows the reader policy below | Preserve the existing one-writer refusal and draft. Do not introduce a hidden send queue. |
| Definite refusal/not sent | Error stays beside that submission and its recovery action | Keep text/files; a new send is available only when non-delivery is definite and the adapter permits it. |
| Unknown outcome/timeout | Say delivery unknown and offer Check delivery | Keep the original operation/input. Never automatically replay; inspect status/transcript or use the existing same-ID board reconciliation. |
| New question revision | Say question updated; show the new version for review | Keep the old draft bound to its revision until explicitly reviewed. |
| Transcript unavailable | Keep prior readable content with an unavailable/stale notice | Read retry never dispatches a turn. Unknown is not an empty conversation. |

The submission row is a projection of the existing durable request/turn/answer record, not a second transcript database. Where native receipts cannot correlate a submitted message safely, retain the single delivery row until verified instead of inventing a matching native message. Keep correlation IDs in Details. A late receipt updates only its own thread even if another thread is selected.

Archive, Restore, Undo, detach/return, ownership transfer, file validation, Cancel-before-launch and permanent-delete protections remain available. Archive is local TaskQ organization and does not archive/stop a native session. Archived transcripts remain readable and show Restore before replying.

## Scrolling and history implementation

Retain Rust byte-cursor reading and the native transcript as truth. Retain Preact/Signals, Wouter, existing shared composer/answer state and native shell. Reuse the current components, but give transcript scrolling one owner instead of compensating at multiple render layers.

### Scroll ownership

- Menu: one content scroll for hosts, pins, filters and tasks. A compact question disclosure contains ordinary text, not an independently scrollable panel.
- Workspace: one independently scrolling sidebar and one independently scrolling selected-thread reading viewport. These are sibling navigation surfaces; neither contains the other. The composer is a non-shrinking sibling of the reading viewport.
- Give every flex/grid ancestor on the constrained path `min-height:0` and every text column `min-width:0`. Use a three-row grid or equivalent flex layout for heading/reading/composer. Wrap prose, filenames, identifiers, counters and controls. Do not mask a broken layout with root `overflow-x:hidden`.
- Use the native scrollbar and input physics. Keep a visible/OS-discoverable scroll thumb, stable gutter where supported and a Jump to latest control when detached from the end. A custom left scrollbar or minimap is not required by the Codex comparison; the user benefit is orientation and reliable navigation.
- Do not intercept global wheel/touchmove or synthesize parent scrolling. `overscroll-behavior` belongs only where a boundary is intentional, such as independent sibling panes or a real modal. It is not a fix for the wrong reading surface. Standard chaining behavior and boundary containment are documented by [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/overscroll-behavior).

### Infinite history

1. Open the newest bounded range. Obtain stable source identity per visible record, preferably transcript generation/session + byte offset + block ordinal. Array position is not identity. Preserve UTF-8 and complete-line boundaries; partial writes, file shrink/replacement and oversized records stay explicit.
2. Load older content before the top edge is reached, using an [IntersectionObserver](https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API) sentinel rooted in the reading viewport. Permit at most one read per direction; deduplicate overlap; ignore replies from an obsolete conversation/generation. Continue through non-display records/short pages until content fills the viewport or the cursor reaches the start. Keep Load earlier as keyboard/error recovery.
3. Capture the first visible record and offset before prepend. Restore that record at the same visual position after layout. Account for font, width, expanded content and delayed image sizing with [ResizeObserver](https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver). One component owns correction; disable competing browser anchoring only on the surface where manual/virtualizer anchoring owns it.
4. Mount a bounded window of messages with overscan; keep offscreen page cursors and measured geometry in a bounded transient cache. Re-read evicted text from the native source. A long open session must not accumulate the entire transcript in nodes or text arrays. Keep the focused/selected range mounted while it is in use; copy, selection and accessible navigation are acceptance requirements.
5. Compare the simplest bounded page-window implementation with the framework-neutral [TanStack Virtual core](https://tanstack.com/virtual/latest/docs/api/virtualizer), which documents dynamic measurement, stable keys and chat anchoring. Prefer an existing suitable component if found during implementation; the current package has no virtualizer. If page-window code needs a general variable-height measurement engine, use the maintained core with a pinned version after a WKWebView/WebView2 spike rather than building another engine. Do not introduce React or a second UI framework. Dependency choice follows the measured spike, not a speculative rewrite.
6. Follow append only while already at the end. Otherwise preserve the anchor and show New messages / Jump to latest; do not acknowledge events. Coalesce visual updates per frame. Return to the same message after switching away, close/reopen or resize; store an anchor as view metadata and re-read its range. If that source is gone, explain the loss and offer Latest.

A scrollbar represents the loaded/measured history window unless total geometry is known. Do not claim a percentage through unseen history or invent a total message count. Reaching the first source record must remain possible; history is never silently truncated to achieve a performance target.

### Responsiveness investigation

Before optimizing, capture the reported hang on the exact installed binary: input-to-feedback time, native queue/read duration, IPC payload size, render/layout time, mounted nodes and any route/bridge timeout. Use existing profiling; no new telemetry collector or raw conversation logging. Separate a blocked Send from a blocked renderer.

Likely paths to measure are full-pane redraw on each input, repeated layout/scroll restoration, synchronous storage/IPC, transcript page size and loaded-node growth. Change only the measured path. Keep storage saves debounced, thread/render updates local and read/status work asynchronous. A slow save or status probe must not disable navigation, typing or Close. Hidden/closed views need no active transcript reads; reopening reconciles fresh state without sending.

## Refactoring sequence and affected surfaces

These are deliverable slices, not newly filed tasks. Reconcile existing work before dispatch; each slice must end in a usable state and include its own adverse cases.

| Order | User outcome | Code and preservation boundary | Required proof |
|---|---|---|---|
| 0 | Existing functionality is accounted for | Complete the preservation audit F0: baseline source/release, all 72 functional groups, target locations, existing callers/exports/native stores and explicit behavior changes. Reconcile open work; no unapproved removal. | Every retained group has a reachable destination and a concrete regression journey; source/fixture/native evidence are separate. |
| 1 | Agreed layout and scroll rules | Update `taskq.md` Product references first if a decision changes; reconcile `menu.md` M3, `ai-workspace.md` C3/C5/D4, `tray-workspace.md` TW2/TW5, `visual-style.md`, operator acceptance and implementation brief. Keep the frozen reference; show intentional differences. | One representative long question and one PM dialogue at actual size, plus compact/narrow behavior. Owner reviews the changed interaction; mocks remain fixtures. |
| 2 | Menu questions stop obstructing task browsing | `task-dashboard.js/.css`, `anchored-popover.js`, `tray-app.js`; preserve other popover uses, task pins, filters, board and lifecycle controls. | Wheel over every preview/text boundary continues the menu list; one direct thread action and correct return position. |
| 3 | A stable window for reading and answering | `workspace.js`, `conversations.js/.css`, `pm-composer.js`, `workspace-answers.js`, shared answer-card interface. Separate heading/reading/composer; scope feedback to the selected thread and operation. | Fixed input, correct role/destination, no foreign receipts, drafts/caret/IME, stale and unknown-delivery recovery. |
| 4 | Long history remains responsive | `conversations.js`, `transcript.rs`, `dialogue.rs`, `conversations.rs`, native read IPC in `tray.rs`; add record identity/bounded reads only where needed. Preserve one-writer/session authority and native source. | Long-history wheel/keyboard/scrollbar traversal, exact anchors, bounded DOM/cache, out-of-order reads and both native renderers. |
| 5 | Daily PM communication is qualified | Existing browser fixtures/instruments, installed Mac and Windows acceptance, evidence package. Update references/fixtures only for approved behavior. | Two consecutive turns in one disposable PM-capable owner session, both replies visible here; failure and restart cases separate. |

Slice 5 status: Mac claude two-turn journey fails ([#1762](https://github.com/taskqorg/taskq/issues/1762)); codex, restart and Windows Not tested. Evidence: [#1743](review-evidence/1743/report.md), [#1753 re-run](review-evidence/1743/requalify-1753.md).

The shared task workspace is also consumed by DashQ. Inventory its consumers before changing exported answer-card/composer/task controls or bundled CSS; keep compatible APIs and scope local-workspace layout under its root. This review does not authorize a DashQ deployment. No host configuration, permission, board lifecycle, file format migration or automatic send/retry change is needed for the initial layout slices.

## Acceptance and recurrence prevention

Use the existing instruments: `tray-task-list.browser.cjs`, `tray-window.browser.cjs`, `conversations-selection.browser.cjs`, `conversations-render.browser.cjs`, `dialogue.browser.cjs`, answer-card tests/fixtures and Rust transcript tests. Add cases where their present assertions miss the risk; avoid another parallel screenshot harness.

The following are proposed acceptance criteria, all **Not tested for the proposed interface**. Numeric performance values are proposed targets pending baseline and owner acceptance, not measured current results.

| ID | Pass condition |
|---|---|
| Q-A1 | With an open question preview, wheel/trackpad over title, summary, actions and preview boundaries moves the intended menu content in both directions; rows stay reachable. Test the actual gesture, not `scrollTop` assignment. No inner question scrollbar. |
| Q-A2 | At the start, middle and end of long content, Send tools remain inside the selected pane without `scrollIntoView()`. Short input stays at the bottom; a naturally growing long input uses the sole reading viewport without an inner scroll or manual grip. Sidebar and reading viewport scroll independently; header remains stable. |
| Q-A3 | Prepending, appending, expanding content, font change and resize preserve the same visible record/offset within 2 CSS px after layout settles; no delayed second jump. Returning to a previously read thread reloads the containing range. |
| Q-A4 | While reading older text, new output does not move the reader. A visible action reaches latest; only an explicit acknowledgement changes unread-event state. Short pages and pages containing only ignored records still reach earlier history. |
| Q-A5 | A 10 MiB source (existing D-A3), a 100 MiB stress source and 10,000 variable-height visible records remain traversable to the beginning without omissions/duplicates. Mounted nodes/cache plateau under the configured window; report the bound. Include long single messages and the documented oversized-record refusal. |
| Q-A6 | Proposed reference-host target: visible input/selection feedback p95 ≤100 ms; no UI main-thread task >50 ms caused by a normal page append/prepend after warm-up. Capture frame times during continuous wheel scrolling; target display refresh budget, report misses. Report exact host, renderer, scale, fixture, cold/warm state and I/O separately; no hard-coded sleep or “16.7 ms” label counts as evidence. |
| Q-A7 | Question #A cannot display answer #B's receipt, including two questions on the same board, late replies and a thread switch while pending. One submission has one visible status and never dispatches twice on timeout/reopen. Accepted transport is not displayed as agent completion. |
| Q-A8 | Free text, numbered choice, changed question, native writer busy, definite refusal and unknown result preserve identity, revision and independent next draft/files. Navigation/New/Archive/Mark read/Close do not dispatch a turn. IME, Enter/Shift+Enter and focus survive updates. |
| Q-A9 | Keyboard-only reading/answering, screen-reader attribution, text selection/copy and scrollbar dragging work with virtualized history. Do not announce the entire history on each prepend/poll. Secondary controls, diagnostics/watch, files, archive/Undo/delete and detach/return remain reachable. |
| Q-A10 | No horizontal overflow or clipped actionable content at menu widths 320/360 and workspace widths 350/620/940 CSS px, 430/560/740 px heights, fonts 14/16/18/20, both themes and native 1×/2× scales. Include unbroken identifiers, long choices, files, code, unknown and empty states. Check scrollWidth on relevant containers, control rectangles and visual output; root clipping is not success. |
| Q-A11 | Inject delayed/out-of-order/failed reads, partial transcript writes, source shrink/replacement, save failure and missing receipt. The selected thread and draft remain coherent, navigation/Close keep working and recovery cannot replay an uncertain send. |
| Q-A12 | On an explicitly disposable local owner session for each supported runtime, send two PM messages, see the two corresponding native replies, revisit older history, switch threads and reopen. Same native session; no unintended worker/board effect. Qualify restart and one-writer refusal independently. |

Keep cheap identity/state/overflow checks in the selected gate. Run affected browser gestures on changed UI code; use real WKWebView and WebView2 for input physics, native scale, window resizing and owner-facing evidence. A Chromium fixture with `platform:'windows'` is not a Windows runtime test. Native daily-use acceptance is a release criterion for this correction, separate from the repository's existing merge policy; do not silently rewrite that policy.

For each native journey pin source/binary hashes, renderer/OS, usable window area, font/scale/theme and fixture identity. Record Pass, Fail or Not tested per criterion, with a short interaction recording for scrolling/sending and at most ten representative review screens. Screenshots alone cannot prove continuity, wheel routing or absence of freezes.

## Review result and next decision

The compact menu currently does too much reading work, the conversation composer moves with history, and feedback can escape its originating task. Infinite file reading exists; bounded long-session rendering and stable record restoration still need design and proof. Those are the central corrections.

Recommended owner decision: adopt compact question attention in the menu and a fixed composer beside one thread-reading viewport; retain current authority, source IDs and native transcript ownership. Review those two representative views in the pinned visual system before implementation. Virtualizer dependency and exact performance budgets follow the focused baseline/spike; they do not block agreeing on the working layout.

This review does not diagnose the exact runtime hang, qualify the running binary, accept new pixels or prove real PM send/reply. Its deliverable is the source-backed review, preservation map, proposed behavior, implementation order and executable acceptance plan.

## Claude consultation and interactive candidate

The owner subsequently authorized an interactive candidate and technical consultation. [Candidate and scenario ledger](exploration/conversations-usability-candidate.md) and [Claude review with dispositions](exploration/conversations-usability-claude-review.md) extend this proposal. The returned model was Claude `claude-opus-5-5[1m]`, selected by its current CLI default; no model override was supplied.

Refinements: delivery is keyed by the persisted operation and separate from session activity; slow is elapsed time without an automatic retry. Recheck loader progress after each short/empty page, not just on IntersectionObserver callbacks; each read advances its cursor or ends explicitly, under a bounded fill budget. Preserve provider message grouping across seams and invalidate obsolete thread/generation/request epochs. Start production work with a measured contiguous page window and bounded forward IPC; add a general virtualizer only if that window cannot meet the accepted budget.

Native input correlation needs the dispatch boundary and provider evidence; equal text or absence of a transcript record is insufficient proof of delivery/non-delivery. Claude's suggested truncation, oversized-record skipping and unknown-record display are not silently adopted against D1/D4. The current reader already expands a long-record read to its documented limit. The prototype's known-count history and scripted provider events are interaction instruments, not a production transcript parser or runtime qualification.

## Owner input and reading correction

The [v6 review](exploration/conversations-preservation/input-scroll-review.md) refines UX02/Q-A2: Send tools stay reachable while a naturally growing long input participates in the sole reading viewport; there is no textarea grip or inner scroll. It records the cross-section click/disclosure audit, visible question context, forge marks and explicit follow/record-anchor checks. It does not close UX04 production bounds or native Q-A3–Q-A6 qualification. Other preservation, authority and provider requirements remain.
