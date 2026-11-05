# TaskQ operator design package

Approved menu direction and AI window policy; detailed candidate for implementation planning.
Runtime remains governed by [taskq.md](../../taskq.md); its [Product](../../taskq.md#product) section links here as the design-target index. Shared rules live in [specq](https://github.com/taskqorg/specq/); this package holds TaskQ-specific requirements.

Working process: [shared product design workflow](https://github.com/taskqorg/specq/blob/main/design/workflow.md). This package supplies its product decisions, references and evidence.

Feature selection: [TaskQ product principles](../product-principles.md). Preserve board ownership, interchangeable clients and the complete menu-only operator path.

App architecture: [selected foundation and migration](app-foundation.md) records Preact + Signals + Wouter, native data ownership and framework-free shared assets. Adoption is not runtime qualification.

## Start here

Read the root contract first. The current visual reference is [workspace-prototype.html](../../tests/fixtures/design/workspace-prototype.html).

[Shared blocks](shared-blocks.md): parent ownership, compact consumer variants and cross-product fidelity.

[Tasks from findings](finding-tasks.md): shared reviewed creation, operation receipts and independent source verification.

## Current package

[Reports in Conversations](reports-conversations.md): #1977 design checkpoint; merged sidebar, decision cards, five operator paths and six mockup screens. Approved design; implementation evidence and native qualification gaps are recorded in its review ledger.

[Decision register](decision-register.md): remaining choices, numbered alternatives and recommended defaults.

[Existing-agent entry](agent-entry.md): discoverable bootstrap, canonical reports, capability limits and owner-requested observation.

[Live worker session stream](live-worker-stream.md): #1825 experiment; runtime capability matrix, transcript-tail prototype, limits and relay options.

| Material | Purpose |
|---|---|
| [App UI v2](ui-rules-v2.md) | Approved shared 38-check profile; TaskQ routes/values/filters and preservation map; upstream review pending |
| [Menu UI rules](ui-rules.md) | Approved 28-check menu target and source-pinned baseline review |
| [Integrated workspace](../../tests/fixtures/design/workspace-prototype.html) | Earlier Workspace review; its stored AI conversations are superseded by TW4/TW5 |
| [Task decisions and plans](plans-and-decisions.md) | Ask-card answers (workspace placement: TW2), AI-led plans and explicit multi-repository gaps |
| [Dynamic resources](dynamic-resources.md) | Accepted Auto default, optional caps, proposed tuning and migration |
| [Shared resource controls](shared-resource-controls.md) | Accepted DashQ reuse of TaskQ resource UI and policy authority; v1 protocol fixtures; remote apply needs an authorized host adapter and fresh readback |
| [Profiling detail](profiling.md) | Implemented stage token sources, board operation kinds, `taskq profile` export columns and report views |
| [GitHub publication waits](publication.md) | Implemented moved-head, conflict and refused-enqueue handling; status row reason and age |
| [Delivery behavior](agent-behavior.md) | Proposed Local / branch / review PR / auto-merge / direct workflows |
| [Agent settings](agent-settings.md) | Workspace access editor, project overrides, review and readback |
| [Product decisions](menu-product.md) | Approved direction, scenario coverage and candidate boundaries |
| [Emergency recovery](emergency-recovery.md) | Stop/interlock/reconciliation proposal and E checks |
| [Host service](host-service.md) | Proposed one service process per host: taskq.md diff, migration, rollback, panic isolation, task split |
| [Menu](menu.md) | M1–M12 behavior and M-A1–M-A21 acceptance |
| [Tray and workspace placement](tray-workspace.md) | Approved TW1–TW8: one nav bar, panels then tasks in the menu, dialogue/reviews in the workspace, any-runtime AI requests, messages count, archive and permanent delete |
| [AI requests](ai-workspace.md) | Visible-session AI requests (#1427), retro, add project and C7 boundary |
| [Task counters](task-counters.md) | All-state totals, Closed and partial-data semantics |
| [Worker permissions](worker-permissions.md) | Host authority, runtime mapping, settings, #865, migration and acceptance |
| [Visual system](visual-style.md) | Typography, geometry, palette, icon inventory and asset sources |
| [Square summary](icon-summary.md) | I1–I5 precise glyph/graph geometry and unresolved policies |
| [TaskQ UI acceptance](operator-ui.md) | D-A1–D-A8 product checks; general D rules are upstream |
| [Use cases](../use-cases/README.md) | UC-01–UC-17, intersections, open Q/P decisions |
| [Adaptive rationale](adaptive-menu.md) | Stable controls, situational content, prioritization choices |
| [Rendering contract](rendering-contract.md) | Native size, scroll ownership, physical bar grid and data parity |
| [Implementation acceptance](ui-acceptance.md) | Complete feature ledger, native matrix and review gates |
| [Report template](ui-acceptance-report.md) | Builder/reviewer evidence and owner decisions |
| [Deterministic fixtures](../../tests/fixtures/ui-acceptance-fixtures.json) | Expected histogram, height and pixel-grid outcomes |
| [README page plan](readme-plan.md) | Project page order, screenshot refresh and stale rule, use-case rows, tasks A–C |

The editable candidate contains local simulation, not live service access. Detach models ownership inside the preview; native separate windows require implementation. AI attachments support local image preview and validation; no file bytes are sent. Rebuild the integrated fragment with `python3 docs/design/build-workspace-preview.py`; edit its source components, not the generated fragment.

## Related specifications

- [Board labels](board.md): names/colors and migration constraints.
- [Incident reports](incidents.md): fault catalog, reporting, consent and operator states.
- [Capacity](capacity.md): earlier limits/load-gate detail; current runtime authority is taskq.md.

## Change routing

General rules follow [shared-rule improvement](https://github.com/taskqorg/specq/blob/main/engineering/spec-first.md#improve-shared-rules). TaskQ decision: taskq.md first, then its linked product specification, candidate and evidence.
Preserve requirement IDs and current capabilities. Shipped and native proof follow [shared verification](https://github.com/taskqorg/specq/blob/main/engineering/verification.md).

[Icon compatibility](custom-icon.md): Custom removal and saved-view fallback; [I1–I6](icon-summary.md) own the fixed Summary SVG.

[Installation and project locations](setup-and-project-locations.md): agent-led and agent-free setup, Projects folder, exact execution roots and S-A1–S-A8.

- [Standalone agentq onboarding](standalone-onboarding.md): warning, detection, dismissal and setup-session recommendations; no runtime changes.
- [Standalone telemetry](standalone-telemetry.md): local store and retention, export as the one data path, no send without serverq, agent analysis guide and gaps G1–G7; no runtime changes.

