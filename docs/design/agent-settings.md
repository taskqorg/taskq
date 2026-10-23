# Agent settings workspace

Detailed candidate under MP5/P6; no active grant or runtime change. One selected Workspace review layout. [Integrated reference](workspace-prototype.html); [isolated component](agent-settings.html) is a development fixture.

## AS1 Entry and identity

- The review opens with menu and workspace navigation visible. Never present an isolated settings component as the complete product.
- Settings → Agents raises the existing workspace at Agents; permission requests deep-link to their exact host/runtime/project/task.
- Host once in the header. Runtime selector shows availability; retain disabled/not-installed agents. Probe age and unknown state remain visible.
- Per-runtime drafts survive navigation. Model/effort are separate from access; permission edits never overwrite them. This reference leaves their existing values unchanged.
- One current-policy summary: effective profile, source/revision, freshness and existing attempts using an older revision. Never mistake a selected draft for applied policy.

Delivery behavior has a separate [project-scoped proposal](agent-behavior.md); Access does not imply push or merge authority.

## AS2 Edit access

- Compact Read only / Workspace / Full control. Selecting prepares a draft, never applies it.
- Files, Network and Tools disclose exact effective restrictions; unknown or unsupported combinations block Apply with a reason. Additional paths require canonicalization and host review; arbitrary text is not a grant.
- Full without external enforcement displays unrestricted filesystem/tool egress and no runtime sandbox. Disable fictitious network/tool restrictions; explain why. Provider transport remains distinct from worker egress.
- Details preserve unsaved choices when switching presets; review the effective change, including derived values. Do not show a green containment badge from a user checkbox.
- Project exceptions are host-owned and keyed by canonical project identity. Show only exceptions plus Add; omitted projects inherit. Removing an override means return to inheritance, which may broaden access; include it in review.
- Local host override can raise or lower within the admin ceiling; repository/task restrictions still intersect it. Show inherited/effective values and any conflicting request.

## AS3 Review and application

- One review shows host/runtime, exact old→new capability changes and affected inheriting projects/overrides. Existing attempts continue with their recorded policy; narrowing is not an emergency stop.
- Confirm binds policy revision and immutable draft. Duplicate confirm is inert. Changed service revision invalidates review; reload and reconcile, never overwrite silently.
- Distinct states: Draft → Review → Saved / waiting for service → Applied, Failed, or Unknown. Applied requires matching fresh readback. Retry first checks receipt/revision; no blind second grant.
- Show next action on failure, preserve draft, retain previous effective policy until readback proves change. A running old-policy session cannot silently acquire or shed permissions on resume.
- Unchanged policy needs no repeat confirmation. Cancel review returns to draft; discard restores saved values.

## AS4 Reference boundary and acceptance

Preview state is a labelled demo control: selecting Editing, Saved, Applied, Failed, Unknown or Revision changed immediately displays that fixture. It is not a product setting. Ordinary confirmation still distinguishes save from service application.

Reference uses fictional versioned probes and two fixture projects. Editable presets/network, project override, removal, revision conflict, failure and explicit simulated readback exercise the route. Files/tools explain boundaries; adding real roots/tool patterns is deferred until adapters expose a truthful capability catalog. No raw runtime flag editor.

- AS-A1: selecting Full changes draft only; derived network/tools become unrestricted without false sandbox claims.
- AS-A2: exception survives host-default change; removal reviews inherited behavior; task/project constraints are not overwritten.
- AS-A3: switching runtime preserves separate draft; model/effort unchanged.
- AS-A4: Saved stays distinct from Applied; failed/unknown readback preserves prior effective state.
- AS-A5: stale revision refuses apply; repeat confirm cannot duplicate a grant.
- AS-A6: both themes and 20px narrow layout remain readable; all review actions reachable.
- AS-A7: runtime unavailable, unsupported enforcement and request above authority have explicit blocked routes in the production ledger.

## AS5 Version and sign-in

Owner requirement (#1657 R18–R19). Agents shows, per host and per OS user/configuration profile, the runtime identity, observed client version, actual account when known, sign-in state and check age together. Host worker/service versions stay separate from Claude/Codex client versions. Never merge credentials across hosts or profiles; codex-app references a shared Codex login only when the host adapter confirms the shared profile.

- Account versus method: the account is the identity the provider reports. "ChatGPT", "API key" or another auth method alone is not an account; show the method separately, or Account unknown.
- States: Signed in, Checking, Check failed, Unknown, Expired, Rejected and Sign in required, each from a source-backed observation. Check failed and Unknown are check outcomes, not auth failures; Expired and Rejected need an explicit provider/adapter signal.
- Signed in at capture: a stale result keeps its value with its age as "Signed in at <time>", never as current confirmation. An installed binary, a running worker or an account label never proves fresh sign-in.
- Expiry: show expiration or remaining validity only when the provider/adapter supplies it; otherwise Expiry unknown. Last refresh is not expiration. A short-lived access token expiring under supported automatic refresh is not an expired login. A confirmed rejection overrides a cached positive result for that credential/profile until a new qualifying observation.
- Remote hosts stay Unknown until that host's own observation arrives; never copy the local result.
- Detail keeps source, last check, last refresh when supplied, expiry meaning, a safe failure reason and a supported sign-in route. A check never starts a login; reauthentication stays an explicit native user action. Expired, Rejected and Sign in required surface in the compact host summary and the single workspace notification route under existing event/acknowledgement rules.
- Never send tokens, cookies, secret key text or raw auth files to the UI or telemetry. This display adds no credential-reading mechanism and no hard-coded expiry-warning threshold.

Reusable rules already live in shared F1/F3/F5 and permission authority. This page defines TaskQ navigation and policy presentation only.
