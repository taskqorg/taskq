# Menu and Conversations interaction experiment

**Scope correction:** this handwritten experiment does not preserve the complete application feature set. Its passing checks cover the named synthetic interactions only. Use the [72-group preservation audit](conversations-preservation-audit.md) and [current-component review](conversations-preservation/preview/index.html) for the retained feature locations. No omission here is an approved removal.

Prototype requested by the owner on 2026-10-28. [Review and plan](../conversations-usability-review.md) owns UX01–UX08 and Q-A1–Q-A12. The [frozen visual reference](control-panels.html) remains unchanged. [Editable candidate](conversations-usability.html) uses the accepted palette, 16 px default adjustable type and Lucide action symbols. It runs on invented local fixtures and sends nothing to a board or native session.

## Proposed behavior under review

- The task menu has one shared content scroll; opening a question reveals compact inline attention and a direct Conversations action. No question/options scroll in the menu.
- Conversations has a sidebar, stable thread heading, one reading viewport and a sibling composer. Narrow windows show list or conversation. Drafts and reading position belong to the thread.
- Each answer or submitted message owns one delivery outcome; no cross-task receipt feed. Unknown delivery offers reconciliation, never automatic resend. Slow confirmed work stays distinct from unknown delivery.
- Codex and Claude fixtures feed one normalized model and renderer. Adapter-specific event translation remains outside the product view. Provider selection is a review control, not silent session migration.
- Long history is a deterministic 10,000-record fixture with bounded mounted rows. Its source count and estimated geometry are known to the prototype; this is not evidence that real native transcripts have a message-count index. Production cursor/anchor/cache work remains in the plan.

## Scenario ledger

| Scenario | Intended interaction and result |
|---|---|
| Open question | Expand a task in the menu; Open question selects its exact thread. Wheel over the excerpt still scrolls the menu. |
| Answer | Submit source choice 1/2 or free text. One receipt appears at that answer; review controls expose successful or delayed readback. |
| PM conversation | Write two messages to either runtime. Local scripted sending, accepted, working/tool activity and reply states remain clear. |
| Long history | Scroll and drag the thumb through 10,000 variable-height records. Input stays fixed. New output while reading earlier content shows a jump action. |
| Slow reply | Confirmed dispatch and working state persist; elapsed time/activity stay visible. Next draft stays editable; no queued send. |
| Unknown delivery | The submitted input remains with Check delivery. Reconciliation changes its status without a new dispatch. |
| Definite not sent | A local refusal keeps the draft and permits deliberate retry. |
| Agent turn failed | Accepted input remains accepted; a later execution failure is shown separately and is never converted into non-delivery. |
| Question updated | A newer revision blocks choice and free-text sends, retains the draft and requires explicit review. |
| Writer busy | Existing native holder refuses dispatch with the draft retained. |
| History unavailable | Previously read content remains; a read retry does not send. |
| Empty and New | Empty detail has a direct New action. New retains independent context/draft. |
| Archive and return | Recent/Archive selections stay coherent. Archive offers Undo; archived content is readable and requires Restore before reply. |
| Files and input | Text/image attachment chips, refusal of PDF/oversized files, removal, IME and Enter/Shift+Enter remain local. Task answers explicitly remain text-only. |
| Narrow and large text | List/detail navigation, theme/font controls, wrapping and fixed input remain usable. |

## Intentional differences from the frozen reference

Compact question attention replaces the full menu question card. One stable input replaces the composer inside the history scroll. Diagnostics/watch editing moves behind the thread menu. Foreign receipts and permanent helper paragraphs disappear. These changes demonstrate the requested interaction slice. Full feature preservation was not established; the audit records the omitted or simplified functions. The production source and frozen reference remain unchanged.

## Claude consultation

[Native Claude technical review](conversations-usability-claude-review.md) records the actual returned model, the complete response and accepted/rejected advice. Delivery and session activity stay separate; slow work has no automatic retry. Production should start with bounded cursor pages and a measured page window. Provider event shapes, message grouping/correlation and native accessibility need actual runtime fixtures; the candidate's converters are invented protocol fixtures, not native parsers.

## Verification

`node docs/design/exploration/conversations-usability.check.cjs` passed: 19 behavior groups, 40 geometry cells, no page errors. Widths: 320/350/620/1024/1400 CSS px; fonts: 14/16/18/20 px; light/dark. The primary question action and composer remain fully visible in every checked cell. This runs with the existing Playwright setup and the visualize preview renderer.

The interaction checks cover real wheel input over the menu excerpt; source-numbered answers; revision refusal and retained drafts; two consecutive local turns per provider; IME; unknown delivery reconciliation without a new dispatch; known refusal retry; failure after delivery; working/slow state with editable next text; busy writer; history read failure; foreign late receipts; 10,000-record bounded rendering; wheel/End/PageUp, append anchors, thread return, font changes and continued sending in long history; native text selection in mounted content; mixed Recent/Archive navigation; files/rejection/removal; archive Undo; new drafts; narrow list/detail navigation and Reset. A return anchor is compared with the actual click-time reading position, including a switch during scroll momentum.

Defects found and corrected during the checks: pending operation behavior depended on the globally selected scenario; follow-end could override upward keyboard navigation; the reading DOM now has per-thread identity so native scroll motion does not transfer between conversations; new sends in the virtualized fixture now appear in its history; menu question state refreshes with the selected fixture; validation stays thread-local; narrow large-text geometry preserves the primary action. These are prototype corrections, not claimed fixes in the installed application.

[Machine results](../review-evidence/conversations-usability-2026-10-28/prototype-checks.json) and eight screenshots pin the candidate. Visual inspection used `visual-screenlist-qa`: question, revised question/draft, two Claude turns, unknown delivery, slow work, long history, narrow dark 20 px and light theme. Images were inspected, not merely exported. Interaction scripts establish the listed Chromium fixture behaviors; screenshots alone do not prove motion.

Runtime, WKWebView/WebView2 input physics, screen-reader output, arbitrary cross-window selection/global Find, real Codex/Claude dispatch, native one-writer enforcement, restart persistence, native scrollbar dragging, 100 MiB file parsing and production performance remain Not tested. The prototype's local transitions cannot establish those capabilities. The full application gate was not run for this separate prototype; production source, dependency pins and frozen reference were not changed.
