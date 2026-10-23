# Live settings and data-display review

Status: findings and proposed forms for owner review. No runtime implementation or task dispatch.

The owner requests simultaneous visual review of the principal Settings surfaces. The companion `settings-review.html` reuses these captured observations and the existing visual system; it is a static-data interactive proposal, not a live connection. It does not promote the proposed widgets below to approved requirements.

## Evidence boundary

Native read-only UI pass on 2026-10-23, installed `local-install/TaskQ.app`, process 57462, displayed version `4.6.36-dev.1792731555`. Current text preference was 18px, growth Up, menu kept open. Navigation raised/closed the workspace; no settings, answers, archives, service operations, grants or limits were applied. The pass ended on Tasks with the existing unsaved view unchanged. Task/host observations changed during the pass; values below are observations at different times, not one atomic snapshot.

Source correlations use worktree `unscheduled-tasks-queue-4efbc3`, HEAD `99b2ae1cb3d566eef99ee328f6ac4581fa687cb7`. This is not proof that the installed executable was built from that exact checkout.

Ten selected native captures are in `<local-home>/.codex/visualizations/2026/10/22/01a1624d-88a8-76f0-a9df-9e47cd23b786/review-2026-10-23/live-settings-audit/`. They are private local review evidence, not public issue attachments. Other visited screens were inspected live without saving more PNGs.

## Findings: severity, evidence, correction

| Finding | Evidence | Proposed correction |
|---|---|---|
| P1 — Access navigation is a dead end | `01-agents.png`, `02-access-dead-end.png`: Agents → Access and runtime settings opens “Agent settings are not available here. Use Settings → Agents in the tray.” Source: `src/tray-app.js:337`, `src/workspace.js:59`. | Give Agents/Access one actual owner surface. Show runtime availability and requested/saved/effective access with freshness; if an adapter is absent, name that limitation at the entry rather than send the user in a circle. Do not fake effective grants. |
| P2 — Services repeat host truth for every project | `04-services.png`: seven project service groups show the same PID example. Source: `src/tray-app.js:249`, `:445`. | One host-service row with state, freshness and project count; project-specific incidents underneath. Put log/folder/review actions in the host detail. Preserve project-specific incidents and operation scope. |
| P2 — Service identifiers are visually clipped | Live Services and Host maintenance: headers show `Service` and only `a`, `c`, `d`, etc.; AX retains the full project names. | Fix the shared disclosure header layout. The host/project identifier must remain readable before secondary metadata or the disclosure glyph; test actual 18/20px type. Exact CSS root cause is not established by this pass. |
| P2 — Work mode has data but hides the actionable result | `05-work-mode.png`: maximum 8; Claude 1/6; Codex unknown/4; Codex app unknown/no cap; CPU sample omitted, available memory sample omitted, sample 41s; Load gate Ready and Also blocked: access appear separately beneath a large path inventory. | Lead with overall admission and its blocking reason. Below: compact runtime used/cap counters, resource meters with units and sample age. Separate load gate from effective admission. Expand scope to project names/paths; never replace unknown usage with zero. |
| P2 — Projects repeat a historical instruction as primary content | `06-projects.png`: the same lengthy “You asked” paragraph repeats under multiple projects. Source: `src/tray-app.js:354`. | One project row: name, task counters and current admission. Historical request text belongs in expandable context; keep board navigation compact and consistently placed. |
| P2 — Delivery is a repeated text list | `03-delivery.png`: seven `project · auto-merge · revision 0` blocks, each followed by Open Board. Source: `src/tray-app.js:438`. | Aligned project/policy rows with a compact policy mark and board action. Revision and policy authority disclose in detail. Keep publication approval separate from Access; do not introduce a local bypass editor. |
| P2 — About repeats an identical long version seven times | `09-about.png`; `src/tray-app.js:339`. | A compact host/client/service version summary. Group identical versions only where source identity is established; expand project observations and freshness. Do not assume different reported components are one process merely because versions match. |
| P2 — Activity does not provide completed-task browsing | `07-activity.png`: six-bin event graph and long interval labels, no completed-task result list on that surface. | Keep activity a compact timeline with readable interval/detail disclosure. Add the already-required direct Completed task view with period/project/search/outcome/result/history; do not treat closed-event totals as completed-task records. This pass did not establish absence of every possible external history route. |
| P2 — Notifications use repeated large option grids | `08-notifications.png`: Question, Closed, Failed, Attempt gone each repeat four long labels and Send test. | Reuse one compact, accessible segmented notification selector per event, with consistent marks and a secondary Test action. Preserve all four modes and show OS permission separately. No preference or test notification was changed/sent. |
| P2 — Recovery read outcome is not actionable | Live Settings → Recovery → taskq → Read current recovery returned “Recovery read or write unconfirmed; inspect the board and reconcile the same selection”; a second observation was unchanged. | Distinguish read failure, unknown read and write outcome. Offer a concrete Board/reconcile route and retain target/selection. The backend cause and recovery correctness remain unknown; no resume/restart/hold action was executed. |
| P2 — Header navigation accumulates a long Back trail | After the inspection route sequence, returning from Settings to Tasks took 20 Back activations. Header actions move below longer titles; there is no direct Home in the inspected menu. | Keep actions in the first row and a direct Home that restores task context. Do not require traversal of every visited settings screen. This is a navigation observation, not a claim that ordinary users always need 20 clicks. |
| P2 — Host observation has no compact per-runtime form | `10-hosts.png`: sample-remote-host idle; mac unknown/stale; win unknown; reconciliation separately shows board Doing/Review and one occupied Claude slot. | Apply accepted one-line named-host + Claude/Codex counters; expansion shows resources, freshness and reconciliation. Keep processes, slots, reservations and board states distinct. Do not show unknown Codex count as zero or infer host health from task state. |
| P2 — Workspace still exposes technical receipts as primary content | Live Inbox: generic AI request title, full identity string, two rejected correlation receipts. Conversations: long launched explanation, full runtime identity, multiple instructions and separate request composer controls. Header Inbox wraps mid-word despite substantial window width; OS and custom titlebars coexist. | Use compact request/outcome rows, hide technical identities in details, show a clear delivery-error action. Keep one shared composer and agreed custom window chrome; never claim native unread replies without an adapter. Native-only dialogue remains authoritative. |

## Additional owner requirement: worker login control

Source correlation at inspected HEAD: `src/runtime.rs:2257` returns runtime/path/version/login/account/kind/refreshed/reason; `src/tray.rs:1651` flattens observations to text, losing structured fields for widgets. `src/runtime.rs:2149` parses Codex “Logged in using ChatGPT” into a method-like account string, not a verified account identity; `:268`/`:297` track rejected login evidence. No expiry field is emitted by this inspected probe. These are source findings, not a new live auth check.

Preserve typed, sanitized per-host/profile observations for the UI. Add account identity and expiry/validity only through supported source adapters. Retain explicit unknown/unsupported/check-failed states; do not parse display strings to invent expiry. Captured Claude account identity and three client versions are real prior observations. The updated Agents screen labels sign-in as reported at capture, exact Codex account as unknown, and every expiry as unknown. No auth file, key or cookie was read for this review; no login/refresh was initiated.

## Section coverage and required form

| Section reached | Live result | Display target / remaining gap |
|---|---|---|
| Appearance | Current 18px selector and follows-system text visible | Combine text/tray/window in one Appearance surface; current type choices are understandable. |
| Tray icon | Icon/Summary previews visible, Summary selected | Retain preview selection and actual accepted icon assets; no retired Custom mode. |
| Window | Auto/Down/Up/Fixed previews, Up selected, Pin and topmost visible | Retain visual choices; unify Appearance and separate lifetime from z-order. Animation and physical drag behavior were not qualified. |
| Notifications | Four event groups and four modes visible | Compact shared selector, OS state, one test control per group. |
| Activity history | Source-backed event intervals and totals visible | Short interval summary and detail, plus separate completed-task discovery. |
| Projects | Seven registered projects with counters/admission/context visible | Compact project rows; historical request context disclosed. Add-project flow was not started. |
| Agents | Agentq and client versions/identity/freshness visible | Structured runtime rows; access destination currently dead. No setup/grant changes tested. |
| Delivery | Per-project policy/revision and Board links visible | Compact policy rows; effective authority and detailed review preserved. |
| Services/support | Repeated same host process, project incidents and actions visible | Host-first service summary; fix clipped labels. Log/folder/restart/update operations not invoked. |
| About/updates | Tray and repeated per-project versions visible | Source-aware version grouping. Update review/application not invoked. |
| Work mode | Admission capacity, load sample and access blocker visible | Overall admission first, distinct runtime capacity/resource widgets. Edit limits and apply not invoked. |
| Host overview | Three observed hosts, unknown/stale states and reconciliation visible | Accepted compact line + expansion; no synthetic healthy/zero values. |
| Recovery | Selection page visible; one TaskQ read unconfirmed | Clear per-project read state and reconcile action; no recovery executed. |
| Inbox / Conversations | Live request, delivery-error receipts and one native-session record visible | One menu window entry, compact event rows, coherent chrome/composer; native dialogue remains external. No Send/archive/dismiss/detach/session launch performed. |

Emergency stop and service/update application were deliberately not invoked. Separate Review Work mode scope and Review admission launchers were not independently exercised; the Work mode screen and its scope text were inspected. No Windows/DPI/IME qualification, exhaustive acceptance matrix or full worker→answer→continuation flow is claimed.

## Coherent correction packages for subsequent owner/PM review

1. **Find work and return to it:** accepted task/filter/pin/Completed surfaces, one content scroll, stable header with Home, one event-aware workspace entry, no menu Enlarge. Preserve scoped counters and working state.
2. **Understand and control the environment:** structured host/runtime/resource/admission widgets; one functioning Agents/Access route; host-first Services; compact Projects/Delivery/About; unified Appearance and notification controls. Preserve real authority, unknown states and review/readback boundaries.
3. **Act on messages and failures:** shared workspace chrome/composer, compact request/receipt rows and actionable delivery/recovery states. Preserve native-only dialogue, request identity, drafts and explicit acknowledgement. Backend recovery/read/receipt causes need separate evidence before implementation.

These are review outcomes, not newly filed or started tasks. Acceptance of the latest views does not certify the current native application.
