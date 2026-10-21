# AI requests

Design under [R3/R9](../../taskq.md). [Menu M7](menu.md#m7-ai-entry) · [Use cases](../use-cases/README.md).

Owner decision (#1427): a tray AI request opens a new visible session, like setup ([#1426](standalone-onboarding.md#contract-detail)). There is no operator turn, no conversation storage, no proposal or `rev` confirmation and no request log. The session does the work itself and may act as PM; the owner reads, answers and archives it in Claude.

## Contract detail

- Request: the tray Ask AI composer names one registered project (default: the first). Send writes `<install>/setup/<id>.json` with `root` = that project and `brief` = the owner's text inside the fixed request template in `src/setup.rs`, then pokes the host. Empty text or an unregistered project writes nothing.
- Launch: the host service launches each request once through the setup path (`setup::step`), with the project root as working directory; brief and launch log stay in `<install>/setup/<id>/`. States, no-start reasons (`runtime missing:`, `sign-in needed:`, `access refused:`), `interrupted` on an unknown outcome and recovery are the [setup contract](standalone-onboarding.md#contract-detail).
- Capacity (owner): an AI request is opened for the human. It takes no § 3 grant; runtime limits, `admission.max`, the emergency hold, Stop, restart, the load gate and launch spacing count only queue workers and reviewers.
- Tray: under the composer the newest AI request shows its kind, reason and next step; `Open session` opens its Remote Control link. Draft and the last 20 distinct sent texts live in `<install>/tray-requests.json`; picking one fills the input only.
- Retro: `taskq retro` writes the same request for the current project with the retro template: read the use-case Proposals and the board's `ask`/`answer`/`requeue` history, give numbered recommendations, and file the ones the owner accepts with `taskq add`.
- Add project (M10): `Set up with AI…` for a new or plain folder writes a setup request (`service setup`) with that folder as its only target.
- Owner comments on `ask` tasks are plain comments; `fold` ignores them (§ 2).
- Leftover `<root>/.taskq/requests/`, `requests.log` and `retro.json` from older versions stay on disk, unread.
