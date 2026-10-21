# Standalone agentq onboarding

Design for #1184, extending [setup](setup-and-project-locations.md) and [menu](menu-product.md) MP7. The owner requested a yellow tray warning when agentq is absent, a cache-cost explanation and one OK to start an install session. These are product targets, not shipped behavior. The recommendations below await owner acceptance; this document grants no installation or runtime permission.

## Choices

| Choice | Options and trade-offs | Recommendation |
|---|---|---|
| Detection | Launcher existence alone is cheap but cannot prove service health. Fresh service readback distinguishes absent, stopped and unknown without launching a model. | Bounded local readback of the standard native install and service status. |
| Session | The ordinary operator is tool-free and cannot install. A queued worker requires a project and admission. A dedicated setup session works before project registration but needs a qualified host launch adapter. | Dedicated, visible setup session launched only by the host service; no queue task. Service-only decision; each OK opens its own session and takes no host slot ([Contract detail](#contract-detail)). |
| Runtime | Automatically pick any installed client for fewer controls, or show the actual eligible runtime before OK for explicit scope. | Show an eligible Codex CLI first, otherwise Claude Code; allow selection before OK. Preserve its model, effort and permissions. |
| Dismiss | Until next launch repeats the warning; forever can hide a later removal; until the next verified install/removal cycle avoids both. | Persist dismissal for this absence episode; retain the section and explicit setup action. |
| Success | A successful session exit is easy but can conceal a failed service. Fresh independent readback proves the running state. | Clear the absence badge only after verified running readback; show setup failures separately. |

Codex-first is a proposed tie-break for this flow, not a change to task-worker runtime preference or existing runtime settings. No eligible client means an actionable unavailable state, never an enabled OK that cannot launch.

## Detection and badge states

A host means the native OS/user installation serving this tray. Windows and WSL are separate; a WSL agentq cannot satisfy a native Windows tray. No recursive discovery, PATH scan across unrelated directories, board request, credential read or AI turn is needed for detection.

The proposed host adapter reads only the known agentq launcher and `<agentq install>/status.json`, using agentq's service-status semantics: a status older than 90 seconds is unknown. A running claim also needs live process identity matching the service executable and reported PID; a PID alone is insufficient. Validate the status shape and version compatibility. Recheck on menu open, after setup completion and at the existing tray refresh cadence. Never spawn repeated service commands on each render.

| State | Trigger and visible outcome |
|---|---|
| Shown | Both known launcher and status are confirmed absent, with no read error. Add one yellow warning mark to Icon / Summary / Custom and an Attention section headed `Save on repeated context`. Accessible label: `agentq is not installed`. |
| Dismissed | Owner selects `Not now`. Hide only this suggestion's badge across tray restarts on this host/user; retain `agentq not installed · Set up…` in Settings → Agents. Ordinary refresh and TaskQ update do not reset dismissal. |
| Starting / running setup | Each OK records a new setup request and opens a new visible session. Section says `Setting up agentq…` with `Open session`; closing a window does not cancel. Existing absence warning remains until readback succeeds, unless dismissed. |
| Agentq running | Compatible fresh status and matching live service identity. Remove this suggestion and its dismissal marker. Settings shows `agentq running`; this proves service liveness, not savings or every client integration. A later confirmed removal creates a new absence episode. |
| Installed, not running | Launcher exists and a bounded service check confirms stopped. Show `agentq installed · service stopped` with `Review setup…`; do not offer a second install as though absent. A stale status alone cannot establish stopped. |
| Unknown | Read denied, malformed/incompatible status, stale data, failed process collection or contradictory observations. Show `agentq status unknown` and the reason with `Check again`; do not claim absent/running or clear the warning as success. |
| Failed / interrupted | Session fails, launch fails, or tray restarts without a known session outcome. Preserve session reference and report partial effects unknown until readback. Explicit retry follows reconciliation; no automatic replay. |

Dismissal affects only the optional absence suggestion. It never hides another warning, a setup failure or unknown status. Removing the absence mark never clears task/host warnings. Use the existing warning composition and readable text, not color alone; preserve queue counters and emergency controls.

Store dismissal and the setup request/session reference in host-owned tray preferences/history, not the board or project task state. The future R1/§4 amendment must name the storage and recovery semantics before implementation.

## Section and one-OK boundary

Suggested English copy:

> **Save on repeated context**
> agentq can reduce repeated context and preserve cache reuse, which may lower model costs. Savings depend on your client and workload; no saving is guaranteed.
>
> Set up agentq on this host for **[selected client]** using **[runtime]**. This installs its verified release, engines and host service, and connects that client's supported configuration, hooks and MCP. Existing sessions keep running. New sessions may be needed to verify the connection.
>
> **OK — set up agentq** · **Not now**

Bracketed fields are populated from bounded runtime capability inspection before enabling OK. Show unavailable/sign-in-needed/unknown separately. Name the host and native platform; show the selected client and editable eligible runtime without introducing another confirmation screen. No unselected client, WSL installation, telemetry credential, DashQ enrollment or legacy removal is included in this OK.

One OK authorizes the displayed effects and starts the session; it does not promise unattended completion. The agent asks only when inspection reveals additional effects, conflicts, legacy removal or missing access. It must not widen permissions, silently fall back to Full, install an AI runtime or obtain credentials. Detecting no eligible runtime preserves `Not now` and offers the official agent-install phrase/link to copy into an existing client.

## Session and full brief

The dedicated setup adapter must work without a project service. Use the existing runtime discovery and visible session-link mechanisms, but not the tool-free R3 operator. Run from an owned setup directory with explicit installation and selected-client targets. No home/project search, project adoption, board token or queue mutation. Each OK opens a new session, like a new Claude or Codex chat; the owner archives finished ones. Runtime authentication and required access are checked before launch; no runtime/model/effort/permission changes are implied.

The complete proposed brief follows. The adapter substitutes the five bracketed values from the exact approved request; these are data fields, not model-inferred defaults.

```text
Set up agentq from https://github.com/taskqorg/agentq on this host.
Host: [host]
Native platform: [platform]
Selected client: [client]
Setup working directory: [owned setup directory]
Approved targets: [exact agentq install and selected-client configuration roots]
The owner selected OK in TaskQ after reviewing installation of the verified
agentq release, its engines and host service, and supported configuration,
hooks and MCP for the selected client only.
Read the official INSTALL.md and agentq.md before acting. Follow the current
release procedure and verify checksums; stop on verification failure.
Run read-only onboarding discovery first. Consent already names the selected
client; do not ask again for the same displayed effects. If the discovered
plan needs other clients, legacy removal, foreign optimizer/provider changes,
new access or other effects, explain them and ask before those changes.
Preserve existing files, ownership, logins, models, effort and permissions.
Do not search home, the disk or unrelated projects. Do not change TaskQ
configuration, admission, runtime policy or board state. Do not kill or restart
existing agent sessions. Do not install another agent runtime or silently
retry with broader permissions. Never request or print credentials in chat.
Do not enroll in DashQ or configure optional telemetry credentials.
Install and onboard only the selected client within the approved targets.
Read back the installed version and native service status. Check the supported
client integration using the official procedure; use a new selected-client
session where required. Report configured separately from verified by a real
turn; report unavailable checks and partial effects explicitly.
Finish with version, selected client, service state, verified integration,
conflicts and remaining actions. A successful process exit is not proof of
installation, savings or badge clearing. TaskQ independently rereads status.
```

The installer owns configuration journaling and restoration. Failure does not delete an existing install or retry onboarding automatically. Recovery inspects the recorded effects; removal uses agentq's official dry-run/uninstall review, never recursive cleanup by TaskQ. Cancellation stops future setup work only after acknowledgement and never implies rollback.

## Scenarios and implementation handoff

Source: owner requirement in #1184 and [use-case intake](../use-cases/README.md). SO-01 through SO-05 are proposed scenario coverage, not runtime evidence.

| ID | Initial state → action → observable result | Exception to preserve |
|---|---|---|
| SO-01 | Standalone TaskQ, agentq absent, eligible runtime → open tray, OK → one visible session installs for the named native client; fresh service readback clears badge. | No project service/board required; a second OK opens a second session. |
| SO-02 | Absence warning → Not now, restart tray → badge stays dismissed; Settings can reopen setup. | Other warnings remain; verified install clears dismissal, later removal shows a new warning. |
| SO-03 | Agentq installed or status unreadable → open tray → stopped or unknown with reason and review/recheck action. | Stale status and a reused PID never count as running or absent. |
| SO-04 | Setup launch/auth/download/configuration failure → inspect/retry → preserved session/effects and reconciled explicit retry. | Tray restart, window close and partial install do not trigger another installer. |
| SO-05 | Native Windows with WSL agentq, or macOS host → OK → only that native host/client is targeted. | No eligible client, legacy install or access conflict stays actionable without silent grants/removal. |

R1, § 3, § 4 and § 8 now name the host setup session; its rules are the [Contract detail](#contract-detail). The § 8 path boundary admits only the request's approved targets; R3/R9 defaults are unchanged. Reuse the approved setup flow, runtime capability inspection and tray warning renderer; do not add a second installer.

Before shipping, qualify SO-01–SO-05 on native macOS and Windows, including zero-project startup, dismissal readback, stale/corrupt status, repeated OK, interrupted installation, client conflict and denied access. Capture actual-size badge/section pictures with other warnings and long host/client names for owner visual acceptance. Verify installed release and service independently; qualify client integration and savings separately. No native/UI/install checks were performed by this documentation task.

References inspected for this design: agentq [INSTALL.md](https://github.com/taskqorg/agentq/blob/main/INSTALL.md), [contract §1/§4/§5](https://github.com/taskqorg/agentq/blob/main/agentq.md) and [README](https://github.com/taskqorg/agentq). They differ on doctor availability and legacy confirmation; use the selected release's documented capabilities and retain explicit legacy consent. Do not make badge detection depend on a proposed doctor command or infer measured cache savings from service status.

## Contract detail

Service-only decision (#1399): the setup session is a host service launch, not a tray, CLI or operator launch. Owner decision (#1426): any setup request opens a new session that finishes it, like a Claude or Codex chat; any session can act as PM; the owner archives it after. There is no one-request slot. Detail for taskq.md R1, § 3, § 4 and § 8:

- Scope (owner): with zero registered projects the session is a full PM chat, not a wizard. It can set up or create a project, show telemetry and show the service; agentq setup is one of its offers. Every external effect still needs § 6 confirmation and stays within the approved targets.
- Storage (R1): each request is `<install>/setup/<id>.json`: `schema`, `id`, `created`, `host`, `platform`, `client`, `runtime`, `targets`, `brief`, `state`, `job`, `session`, `url`, `reason`. The session runs in `<install>/setup/<id>/`, which holds its input and log. No board, project or queue state is read or written.
- Request: `taskq service setup [--runtime claude] [--client C] [--target PATH..] [--json]` needs no registered project and no board token. Each call writes a new `pending` request and pokes the host. Targets are absolute existing paths. The brief is fixed at request time from the template in `src/setup.rs`, the agentq brief above inside the PM chat scope. No model, effort or permission field exists: launch resolves § 8 Access with host.json `permission_mode`.
- Runtime: a Claude background session with Remote Control gives the visible session link. Other runtimes are refused at request time until a visible adapter is qualified for them.
- Launch: only `service run` launches, oldest `pending` request first, each once. The host writes `launching` before the native call, then `launched` with the native job, `interrupted` with the reason when the outcome is unknown, or `failed` on a confirmed no-start.
- Capacity (owner): a setup session is opened for the human, so it takes no § 3 grant. Runtime limits, `admission.max`, the load gate and launch spacing count only queue workers and reviewers; Claude capacity 0 does not hold setup. Only the emergency hold and a pending Stop or restart hold a `pending` request.
- Ownership: after `launched` the Claude session manager owns the session. TaskQ reads the job's session and Remote Control link once at launch and never tracks it after; ending or archiving it is the owner's act in Claude.
- No-start kinds: before the native call the host checks that `claude` is on the service PATH and not logged out; a missing binary, `not logged in`, a § 8 access refusal or Claude's `Workspace not trusted` writes `failed` with reason prefix `runtime missing:`, `sign-in needed:` or `access refused:`. taskq never trusts a folder or widens access itself (R9).
- Tray (#1400): the tray reads the newest request only and shows one setup row: `waiting`, `starting`, `launched`, `runtime`, `login`, `access`, `failed` or `interrupted`, with the reason and the owner's next step. `Open session` opens the recorded Remote Control link, where the owner reads output and answers; the tray never relays text itself.
- Recovery: a service start that finds `launching` writes `interrupted`. No request relaunches; a new `service setup` opens a new session.
- Exit: `launched` means only that the session started. agentq running and verified integration need independent readback (#1391).
