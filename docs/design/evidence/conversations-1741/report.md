# Conversations slice 3

Implements [#1741](https://github.com/taskqorg/taskq/issues/1741) against baseline `b8427548779bd6a70555b6dc2bb18023d1d98bc4` and the approved [D4 input/reading behavior](../../ai-workspace.md#workspace-dialogue-d1d8). No product rule is amended (R13). Native conversation acceptance remains **Not tested**, reserved for slice 5 (R12).

## Failure and correction

- Before: `node tests/conversations-stable.browser.cjs` failed Q-A2 at 940 px / reading start: Send was below the visible pane. It also failed Q-A7: switching to another question on the same board exposed the first answer's receipt/toast.
- After: the same check passes Q-A2, Q-A7, Q-A8 and Q-A11. Heading and Send tools are siblings of the sole reading viewport. The short editor sticks to its bottom; a long editor joins its natural flow, using the approved reference's 45% threshold. An invisible measuring field preserves the live field while growing or shrinking it.
- Task feedback filters board, task and current operation; the workspace presents one inline receipt. The menu's global Answered history/toast remains unchanged. Owner-operation notices and pending/recovery status stay with their conversation.
- Tests previously scrolled Send into view before checking it. The updated geometry checks inspect its actual bounds without that workaround. Existing state, keyboard, validation, file, draft and same-operation recovery assertions remain. The menu history/toast is still protected by `tests/tray-answer.test.cjs`.
- The event-free-task restart instrument could read an older navigation write with the same selection but an empty opened-task list. The baseline reproduced `0 !== 1`; `messages.browser.cjs` now waits for the selected identity and both opened tasks before simulating restart. Its restoration and no-replay assertions are unchanged.

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

## Local gate limitations

The first canonical Windows run stopped after 422.875 seconds: 530 Rust cases passed, 20 failed and 36 were ignored. Unchanged host-policy, installation, lifecycle and native-security fixtures encountered the worker authority boundary (`TASKQ_ATTEMPT`), including `a worker cannot change host policy`. These host checks are **sandbox-blocked**, not Pass; the worker identity and guards remain intact (§7/R9/R12). Two incident fixtures also inherited this run's diagnostic opt-out; their isolated fake-transport group passed all nine cases without that override. Offline selftest passed 99 fold traces, formats and private staged capture; its board read was explicitly unknown.

The remaining Node checks caught the intended `workspace-answers.js` dependency change in the published workspace source manifest. Its hash was regenerated with `node tests/task-workspace.test.cjs --write`; the unchanged provenance assertion then passed. This updates source provenance, not the shared schema or exports. Final component/browser results and remaining host qualification are recorded in the task result; green `merge_group` CI remains the publication gate.

## Requeue correction: operation results and recovery

PR #1745 review found an A3/A4 preservation gap: Archive, Delete and New from draft changed selection but wrote their completion notice under the previous identity. Delete could therefore reveal the next thread's stale Archived notice. A native save/action failure also held the shared queue while other threads had no visible source or recovery target.

`conversations-stable.browser.cjs` now reproduces each defect at the actual workspace bundle boundary. Before the correction all four new journeys failed: Archive showed only Undo, Delete and Fork had no completion text, and the other thread could not identify the blocked operation. After the correction all eight journeys pass. Delete includes an archived successor with an older Archived notice; its new result replaces that notice.

The three selection-changing actions now write their result to the resulting selection, including an empty list. A blocked thread names the failed conversation and exposes the existing open control; that control reveals the original draft, reason and Retry action. Opening it does not retry the operation or dispatch a turn. Ordinary answer/reply receipts retain their originating thread and operation identity. No contract decision changes (R13); this restores existing A3/A4 capabilities.

The adjacent Conversations and selection instruments also pass with their existing draft/caret/files, IME, independent-list, Archive/Undo/Restore, refusal and no-send assertions. Recovery controls remain inside the workspace at 350/620/940 CSS px with no horizontal overflow. [Correction capture and exact environment/source hashes](review-fix/manifest.json) show the actual browser bundle; [cross-thread recovery](review-fix/cross-thread-recovery.png) was inspected at natural size using visual-screenlist-qa. The selected draft, disabled Send and named recovery target are visible without clipping. This is Chromium evidence, not WKWebView/WebView2 conversation acceptance (R12; native remains **Not tested**, slice 5).

Canonical gate and refreshed README snapshot results are recorded in the task result (§7). No deployment, host-policy change or native send is part of this correction.
