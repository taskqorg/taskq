# AI requests

Design under [R3/R9](../../taskq.md). [Menu M7](menu.md#m7-ai-entry) · [Use cases](../use-cases/README.md).

Owner decision (#1427): a tray AI request opens a new visible session, like setup ([#1426](standalone-onboarding.md#contract-detail)). There is no operator turn, no conversation storage, no proposal or `rev` confirmation and no request log. The session does the work itself and may act as PM; the owner reads and answers it in its runtime.

Approved target (#1462, not shipped): the composer moves to the workspace and a request opens a session in any available runtime, Claude or Codex, named in the request UI ([TW4](tray-workspace.md#contract-detail)). Messages, archive and permanent delete act on TaskQ message records only ([TW5–TW7](tray-workspace.md#contract-detail)); no TaskQ conversation storage returns.

Owner-approved #1464 scope ([answer 6319522673](https://github.com/taskqorg/taskq/issues/1464#issuecomment-6319522673)) accepts the seven-image layout and the Claude-only handoff; native workspace/Codex qualification remains release work. Full replies stay in the native runtime. #1580 implements the separately approved draft/file workspace below; no conversation store or proposal protocol returns.

Owner-approved #1467 scope ([answer 6328516821](https://github.com/taskqorg/taskq/issues/1467#issuecomment-6328516821)) accepts M's seven-image layout and message-only outcome. Combined #1465/#1466 acceptance and native workspace qualification remain separately gated.

## Native dialogue workspace (C1–C9)

The [#1580 owner answer](https://github.com/taskqorg/taskq/issues/1580#issuecomment-6343852087) selects native-only dialogue. A1/A7 mean workspace drafts, files, request history and Open session, without an in-workspace reply chain. R3 and TW5–TW7 retain authority. The archived design's operator, transcript, proposal protocol and active-only archive guards do not return.

- C1/C2: Conversations lists TaskQ-known owner drafts/requests only. Work sessions are a distinct collection using R6 links. History contains the last 20 submitted texts and fills only. There is no scan of unrelated native chats.
- C2/C7/CW1–CW8: New clears search and focuses its own draft. The first draft line is a temporary title; an explicit name wins. Each draft retains project, text, caret and record scroll. Recent and Archive retain their own selections. Clear search keeps selection. Archive any item, stay in the list, choose another and offer Undo; Restore/Undo opens the draft. Close hides, never sends, cancels or archives. Pin, enlarge/restore and resize use the native window. Explicit Detach/Return transfers one editable owner; reopen raises that owner. Restart restores metadata when opened and returns ownership to main without reopening detached windows.
- C3: Selection/drop reads explicit files, never source paths. UTF-8 text/Markdown and PNG/JPEG/WebP: at most 4 files, 5 MiB each, 20 MiB per draft and 100 MiB of owned copies per installation. Empty files, binary text, directories and executables are refused. Image selection rejects unreadable data and dimensions above 8000 pixels per side. PDF is unsupported (0 pages): no qualified page-count validator. Draft text is at most 64 KiB; local metadata stops at 8 MiB. No truncation or automatic history eviction. Show these limits before selection and preserve other files/draft on refusal. Image preview uses local bytes only.
- C3/C7: Native storage owns copies in `<install>/setup/<request-id>/attachments/`, keyed by SHA256. Send freezes ID/hash/length and verifies bytes before handing paths to the native session's Read tool. File contents never enter command history or telemetry. Remove affects only the draft reference. Permanent deletion removes owned copies and metadata, never source files/native chats; deletion fences prevent resurrection.
- C4: One immutable launch request per item. Native preference revision fences stale saves; Send persists its immutable input before dispatch. Unknown/partial delivery retains draft/files; Check delivery reads that same ID without replay. New/selection/reload/detach/close never dispatch. A next draft is editable while launch waits; an explicit New from draft creates a separate owner request. Continue replies through Open session. Cancel acknowledges only a pending launch under the setup lock; launching/launched/unknown requests require the native runtime. No enabled runtime Stop without an adapter.
- C5/C6: Board questions keep source numbers and exact-revision receipts in Inbox; full replies and operation review remain in the native runtime/existing reviews. Work-session targets use #1565/R6; missing/remote capabilities stay explicit. This workspace adds no gateway, scheduler, grants or task store.
- C7/C9: Extend `<install>/tray-requests.json` for local draft/view metadata, preserving existing request/answer/history fields. Corrupt or unavailable storage refuses writes and leaves existing bytes. Durable setup records remain launch truth. Never infer runtime completion from `launched`. Archive and permanent deletion follow TW6/TW7, including the prelaunch deletion refusal and no trash/undo for Delete.

## Acceptance (C-A1–C-A10)

| ID | Native-only evidence |
|---|---|
| C-A1 | Two items keep separate context, draft, caret, files and request IDs. Replies remain native. |
| C-A2 | Reopen/detach/return/close never sends and only the owner can edit. |
| C-A3 | History fills only; item selection restores; session selection opens a verified target. |
| C-A4 | Select/drop/remove/preview/rejection and failed native storage preserve drafts; sent bytes/hash are immutable. |
| C-A5 | Exact-revision saves and immutable same-ID launch input refuse stale or conflicting writes. Existing board-answer guards remain. |
| C-A6 | Partial launch/save readback stays unknown; reconciliation cannot duplicate a launch or board apply. |
| C-A7 | Restart/offline preserves drafts/files; only acknowledged prelaunch cancellation says Cancelled. |
| C-A8 | Work sessions use the R6 host/runtime target, with missing/unsupported destinations explicit. |
| C-A9 | UC-01–UC-07 use visible native owner sessions/current reviews; unsupported remote/headless routes remain explicit. |
| C-A10 | 16/20 px, both themes, narrow/short layouts, keyboard/IME, accessible names/live status and reachable composer/Back. Native screen-reader proof stays separate. |

## Contract detail

- Request: the workspace Ask AI composer names one registered project (default: the first). Send writes `<install>/setup/<id>.json` with `root` = that project and `brief` = the owner's text inside the fixed request template in `src/setup.rs`, then pokes the host. Empty text or an unregistered project writes nothing. The workspace names the qualified Claude Code runtime; the any-runtime target remains unqualified. A stable request ID binds the project, runtime and literal request. Check delivery reads that same setup record, including after restart; it never creates a new-ID retry. A lost or partial receipt preserves the draft and shows unknown until readback.
- Launch: the host service launches each request once through the setup path (`setup::step`), with the project root as working directory; brief and launch log stay in `<install>/setup/<id>/`. States, no-start reasons (`runtime missing:`, `sign-in needed:`, `access refused:`), `interrupted` on an unknown outcome and recovery are the [setup contract](standalone-onboarding.md#contract-detail).
- Capacity (owner): an AI request is opened for the human. It takes no § 3 grant; runtime limits, `admission.max`, the emergency hold, Stop, restart, the load gate and launch spacing count only queue workers and reviewers.
- Workspace: under the composer the newest AI request shows its kind, reason and next step; `Open session` opens its Remote Control link. Draft and the last 20 distinct sent texts live in `<install>/tray-requests.json`; picking one fills the input only.
- Retro: `taskq retro` writes the same request for the current project with the retro template: read the use-case Proposals and the board's `ask`/`answer`/`requeue` history, give numbered recommendations, and file the ones the owner accepts with `taskq add`.
- Add project (M10): `Set up with AI…` for a new or plain folder writes a setup request (`service setup`) with that folder as its only target.
- C7 (target, TW6/TW7): archive any message entry; delete one or all archived ones permanently. Delete removes only TaskQ message records and owned attachment copies, refuses a request not yet launched, and never stops a session, changes a task or touches source files or native chats.
- Owner comments on `ask` tasks are plain comments; `fold` ignores them (§ 2).
- Leftover `<root>/.taskq/requests/`, `requests.log` and `retro.json` from older versions stay on disk, unread.
