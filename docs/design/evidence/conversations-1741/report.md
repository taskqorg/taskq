# Conversations slice 3

Implements [#1741](https://github.com/taskqorg/taskq/issues/1741) against baseline `b8427548779bd6a70555b6dc2bb18023d1d98bc4` and the approved [D4 input/reading behavior](../../ai-workspace.md#workspace-dialogue-d1d8). No product rule is amended (R13). Native conversation acceptance remains **Not tested**, reserved for slice 5 (R12).

## Failure and correction

- Before: `node tests/conversations-stable.browser.cjs` failed Q-A2 at 940 px / reading start: Send was below the visible pane. It also failed Q-A7: switching to another question on the same board exposed the first answer's receipt/toast.
- After: the same check passes Q-A2, Q-A7, Q-A8 and Q-A11. Heading and Send tools are siblings of the sole reading viewport. The short editor sticks to its bottom; a long editor joins its natural flow, using the approved reference's 45% threshold. An invisible measuring field preserves the live field while growing or shrinking it.
- Task feedback filters board, task and current operation; the workspace presents one inline receipt. The menu's global Answered history/toast remains unchanged. Owner-operation notices and pending/recovery status stay with their conversation.
- Tests previously scrolled Send into view before checking it. The updated geometry checks inspect its actual bounds without that workaround. Existing state, keyboard, validation, file, draft and same-operation recovery assertions remain. The menu history/toast is still protected by `tests/tray-answer.test.cjs`.

## Acceptance and preservation coverage

The [source audit](../../exploration/conversations-preservation-audit.md) remains the capability inventory. This table maps the touched groups to runnable browser journeys, not native qualification.

| Acceptance / audit groups | Journey and check | Observed boundary |
|---|---|---|
| A1; C20, C24 | `conversations-stable.browser.cjs`: long question at start/middle/end, 350/620/940 px, grow/shrink, stable heading, fixed Send, no inner field scroll. `dialogue.browser.cjs`: prepend, append, expanded Status and long replies. | Chromium geometry and synthetic transcript receipts |
| A2; A07–A10, C22 | `conversations-stable.browser.cjs`: same-board switch while answer pending, late accepted receipt, one local status, no resend. `tray-answer.browser.cjs`: wrong-root receipt, stale/unknown/partial/refused answers and same-ID reconciliation. | Actual shared card and workspace IPC with invented receipts |
| A3; C01–C14, C16–C19, C23–C24, A01–A06 | `conversations.browser.cjs`, `conversations-selection.browser.cjs`, `workspace-communication.browser.cjs`, `tray-answer.browser.cjs`: independent drafts/caret/files, selections, history fills only, archive/restore, names, native links, IME/Enter/Shift+Enter, exact revision/choice/text, owner actions and consequential confirmation. `dialogue.browser.cjs`: writer refusal and Watch controls. | Browser/UI authority boundaries; native storage/provider effects are unqualified |
| A4; C14–C15, C20, A08 | `conversations-stable.browser.cjs`: out-of-order old-thread reads ignored, failed read keeps selected draft, late owner refusal stays on its operation. `conversations.browser.cjs` and `workspace-communication.browser.cjs`: missing receipt/reopen and explicit same-ID recovery without automatic replay. | Synthetic delay/refusal/read recovery; native reader format and lifecycle are unchanged |
| A5; X01, X05–X06 | Existing answer-card and PM-composer exports/source files are unchanged. Their component and browser checks retain the published API; workspace-only layout is scoped beneath `.workspace`. `conversations-render.browser.cjs` retains six boundary/state fixtures without overwriting historical evidence. | Shared bundles/browser; installed DashQ and native OS windows are Not tested |

No capability is removed. Context, file validation/previews/removal, Watch, native-session links, archive/delete confirmation and operational controls retain their existing handlers. Bounded history/cache and native provider/window qualification belong to the other planned slices.

## Visual evidence

[Manifest](manifest.json) records browser version, viewport, font, theme, scale, baseline, source hashes, bundle hashes and image hashes. All images use invented data and the actual workspace bundle; no board requests or native turns were sent.

- [Before: long question hides the editor and Send](before-long-question.png).
- [After: short editor and Send remain visible while reading](after-stable-question.png).
- [After: long draft uses the reader and retains validation](after-long-draft.png).

These three distinct states were inspected at natural size. Width variants are automated geometry cells, not extra review screens. Canonical gate and final commit/PR receipts are recorded in the task result (§7); screenshots do not prove installed conversation behavior or merge-result CI.
