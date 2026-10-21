# AI requests

Design under [R3/R9](../../taskq.md). [Menu M7](menu.md#m7-ai-entry) · [Use cases](../use-cases/README.md).

Owner decision (#1427): a tray AI request opens a new visible session, like setup ([#1426](standalone-onboarding.md#contract-detail)). There is no operator turn, no conversation storage, no proposal or `rev` confirmation and no request log. The session does the work itself and may act as PM; the owner reads and answers it in its runtime.

Approved target (#1462, not shipped): the composer moves to the workspace and a request opens a session in any available runtime, Claude or Codex, named in the request UI ([TW4](tray-workspace.md#contract-detail)). Messages, archive and permanent delete act on TaskQ message records only ([TW5–TW7](tray-workspace.md#contract-detail)); no TaskQ conversation storage returns.

Owner-approved #1464 scope: move requests and task answers to the workspace. Full replies stay in the native runtime. Attachment handoff is deferred to a separately approved task; no conversation store or proposal protocol returns.

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
