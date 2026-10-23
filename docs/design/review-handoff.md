# TaskQ interface review: correction handoff

Owner handoff: 2026-10-23. PM owns reconciliation and decomposition. The owner explicitly accepts the icon set and colors; earlier selections remain binding. This package authorizes PM planning and task creation, not implementation start, deployment, service changes or grants.

Planning task: [#1657](https://github.com/taskqorg/taskq/issues/1657). Current transfer receipts are stored with the package; the issue owns published archive links and the PM plan.

## Start here

Read current taskq.md and specq first. Then read this file, menu-product.md MP11–MP15/P-W1–P-W3/P-V1, visual-style.md, ai-workspace.md and the current implementation brief. Paths below are relative to docs/design/ inside the package.

| Material | Purpose |
|---|---|
| exploration/review-control-panels.html | Connected correction candidate: dashboard, pins, filters, details, history, Settings and workspace |
| exploration/settings-review.html | Twelve Settings routes together; captured observations are labelled, not live values |
| exploration/icon-review.html | Owner-accepted eight-state glyph/color mapping and shared control/identity inventory |
| exploration/review-control-panels-review.md | Revision-specific browser checks, icon audit and remaining implementation gaps; later sections supersede historical conflicting revisions |
| exploration/live-settings-review.md | Actual installed-app route findings, readable-data gaps, source correlations and untested operations |
| review-evidence/live-owner-review.md | Earlier native review, per-acceptance assessment of both deliveries and owner corrections; historical captures retain their own versions |
| review-evidence/source-handoff-1573.md | Original implementation-review brief |
| exploration/control-panels.html | Unchanged historical frozen baseline; current owner corrections override affected behavior |
| evidence/review-package.json | File hashes, baseline/source provenance, included evidence and redaction boundary |

Original planning: [#1573](https://github.com/taskqorg/taskq/issues/1573). Delivered work: [#1578](https://github.com/taskqorg/taskq/issues/1578), [PR #1590](https://github.com/taskqorg/taskq/pull/1590), [#1580](https://github.com/taskqorg/taskq/issues/1580), [PR #1612](https://github.com/taskqorg/taskq/pull/1612). Both delivery issues now read closed; that does not establish that all acceptance is satisfied. Reconcile open work before filing anything. Prefer fixing acceptance gaps within their original outcome; do not create parallel writers or one ticket per icon/widget/check.

## Complete remark coverage

Every row is required scope unless explicitly marked open or unverified. IDs are handoff trace keys, not a second product specification. Requirement homes remain the linked contracts. A prototype Pass never means delivered/native Pass.

| ID | Owner remark / required correction | Canonical home and review evidence |
|---|---|---|
| R01 | Task discovery dominates: compact useful status, remaining space for filtered tasks; remove repeated state/identity prose. | MP11–MP15; connected candidate; live-owner review |
| R02 | Existing task identity anchor looks interactive; details open only on click/Enter/Space. No hover/focus opening, refresh-induced random panels, right ellipsis, boxed action column or extra chevron. | MP13; connected click/keyboard checks; reported hover defect still needs exact native reproduction |
| R03 | Details prioritize the actual question/problem, exact numbered choices, result and relevant actions. Disclose technical IDs, total age, ordered steps and durations. Do not replace real question cases with generic placeholders. | MP13; Q1/Q2; taskq section 2; live-owner review |
| R04 | Preserve O1: current-stage elapsed in the compact row; total age/step count/per-step durations in details. Unknown is explicit; elapsed time alone is not a stall. | MP13; #1573 O1; original lifecycle request retained |
| R05 | Claude/Codex mark immediately before stage time; extra remote mark only for proven remote execution. Verified session target opens directly; unavailable targets are disabled with a reason. | MP13; C6/R6; session adapters remain authoritative |
| R06 | Task dividers are several times quieter: Text at 8% over Surface. Popovers are more distinct: opaque raised surface, separate edge and shadow. | visual-style revision contract; dark/light captures |
| R07 | Stable top row for frequent actions; long section title/breadcrumb uses the row below. No jumping icons or mid-word headings. | MP12; shared window chrome; native header findings |
| R08 | Home returns to the task dashboard in one activation. Keep Back; do not replay every visited page or clear working state. | MP12; observed 17/20-Back inspection paths |
| R09 | One Conversations/workspace menu icon, with deduplicated source-backed new/actionable-event count. Route to the relevant Inbox/Conversations section; opening does not acknowledge. | MP12; accepted single-entry revision |
| R10 | Retain per-view criteria/query/grouping/selection/scroll and independent drafts across Settings, views, refresh, close/reopen and restart. | MP11/O3; C2/C7; restart remains a native proof obligation |
| R11 | Multi-state and multi-project selection; combined/project-grouped display; scoped All count. Fixed Doing→Review→Ask→Failed→Ready→Blocked→Later→Held order, then priority/issue/canonical identity. No reorder arrows/editor or unsolicited drag handles. | MP11; O2/O3; task-counters semantics |
| R12 | One-click Reset to the selected saved version when modified. Working changes apply immediately; only Save changes a preset. | MP11/O3; connected candidate |
| R13 | Direct deletion of custom saved views; built-ins protected. Distinguish shortcut unpin from deletion; cover active/inactive deletion and recovery. P-V1 proposes named persisted Undo, context restoration and no jump after later edits; exact fallback/Undo policy remains a bounded choice. | MP11; P-V1; exercised deletion/Undo/reload fixtures |
| R14 | Global pinned tasks remain visible above results across views, filters and search; normal compact rows, click for actions, no duplicate. Pin closes details. Pinning a task is different from pinning a view shortcut or window. | P-W1; latest owner correction supersedes filter-hidden pins |
| R15 | One shared menu content scroll for overview, pins, filters and tasks. Pins are first in normal flow, not a fixed overlay. No separately scrolling watch/task/settings subpanels. | P-W1; shared-scroll checks; controls/footer stay accessible |
| R16 | Named hosts and separate Claude/Codex running-agent counters visible together in one compact expandable line. Unknown/stale is never zero; global counts must not be labelled as one host. | MP14; native host-scope mismatch; compact overview revision |
| R17 | Expansion shows CPU/memory with units, freshness and resource details. Keep running processes, occupied slots, reservations, capacity and board tasks distinct. | MP14; live-settings review; shared snapshot/read model |
| R18 | Per host/profile show worker/service version separately from runtime-client version, actual account when known, sign-in state, explicit expiry when supplied and check freshness. ChatGPT/API key is a method, not an account. | Worker/runtime sign-in contract; captured Agents sheet |
| R19 | Signed in at capture is not current validity; stale/unknown/check-failed differs from expired/rejected. Automatic token refresh/expiry does not alone imply expired login. Remote values remain unknown until observed. | Worker/runtime sign-in contract; no auth probe/login performed |
| R20 | One Appearance route combines tray icon, font and window behavior. Use direct visible choices/previews for short option sets; retain immediately animated growth previews and reduced-motion alternative. | MP12/M13; Appearance sheet |
| R21 | Retain existing opening behavior: Auto follows actual native anchor/opening direction, never an OS-specific constant. Explicit Up/Down/Fixed and last user preference survive drag/pin. | M13; window settings review |
| R22 | Separate Settings window and Conversations workspace with shared custom chrome/drag region; avoid duplicate system/custom titlebars. Preserve independent lifetimes/context; do not make twelve real Settings windows. | P-W2/P-W3; owner accepted separate windows |
| R23 | Always-visible Close; first real drag from transient mode enables Keep open, then remembers last mode. Closing never archives/cancels/sends. | M13; window behavior checks; native gestures unqualified |
| R24 | Frequent Always on top toggle in the stable action row; overlapping squares with filled foreground square. It controls z-order, independently from Keep open/task pin. | MP12/P-W3; preserve accepted composite icon |
| R25 | No Enlarge in the task menu. Keep integrated corner resize grip, without square button chrome; pointer target and keyboard access remain. Settings/workspace retain Enlarge/restore. | MP12/P-W3; accepted menu correction |
| R26 | Footer shows version only plus corner grip. Remove duplicate host/live/status prose. | MP15/M13 |
| R27 | Browse completed tasks directly, with project/period/search/outcome/result/PR and task history. Separate success, not-planned/cancelled and unknown closure. Board-owned pagination, no new task store. Activity counts are not task history; no mandatory Closed counter is added. | MP14; completed candidate; real history adapter remains missing/unverified |
| R28 | All main Settings routes get compact readable widgets: Settings, Appearance, Hosts/resources, Work mode, Agents/access, Projects, Delivery, Services, Notifications, Activity, Recovery, About. Preserve operational routes beyond this overview. | MP15; twelve-screen sheet; live route inventory |
| R29 | Remove Agents→Access circular placeholder. Show effective/requested/saved/applied policy honestly; use the supported editor or one precise unavailable reason. | MP5/MP15; reproduced Access dead end |
| R30 | Services belong to the host once. Projects retain readable names, registration, incidents and log/action access; no same-PID repetition seven times or one-letter clipping. | MP15; Services native finding |
| R31 | Compact Projects/Delivery/About rows; old request prose/revisions/technical identities in details. Group equal version reports; distinguish installed/running/available and unconfirmed update checks. | MP15; real-data sheet |
| R32 | Work mode shows overall admission first and separate capacity/resource/access facts. Preserve MP2 future-rule versus task-snapshot scope; reconcile the thin existing scope route without disguising it as complete. List filters never change admission. | MP2/MP15; admission blocked despite load-ready observation |
| R33 | Notifications use compact event/mode choices with separate OS authorization and supported tests. No fabricated unread replies or permission grants. | MP12/MP15; Notifications sheet |
| R34 | Activity shows a concise time/units/legend summary and useful detail. Keep event statistics distinct from current task counts and completed records. | MP14/MP15; captured event bins |
| R35 | Recovery shows per-target read state/freshness and relevant next action. Preserve unread/unconfirmed/failed states, selected target and operation fences. Stop/Pause/Recovery remain distinct reviewed effects. | MP6/MP15; unconfirmed recovery read; E1–E5 |
| R36 | Inbox/Conversations show compact request/outcome rows and clear delivery errors; technical correlation IDs disclose on demand. Investigate observed pending-correlation conflicts before retry; never suppress the guard or blindly resend. | MP12; shared input contract; live receipt observations |
| R37 | Preserve #1580 answer 2: dialogue remains native-only. TaskQ owns drafts/files/request history/Open session, not an invented reply chain or unread native transcript. Work sessions remain separate from owner requests. | MP3/C1–C9; native-dialogue decision |
| R38 | New clears hiding search/selects/focuses the new draft; switching restores independent drafts/context. Archive stays in the current list and offers Undo; Restore/Undo and Recent/Archive selections work. Closing is separate. | C2/C7; CW1–CW8; closing walkthrough; real native journey still needs qualification |
| R39 | One shared composer for supported text/file entry, attachment chips, validation, context and send. Preserve exact numbered answers and transport capability; unsupported PDF/file-answer/native reply routes remain explicit. | C3/C4; review input contract; no transport invented |
| R40 | Questions are coherent from producer→parser→form→answer→dispatch→receipt, including unsupported forms, stale revision and partial/failed delivery. Review actual task examples, not just visual buttons. | taskq section 2; Q1/Q2; exact-revision controls; end-to-end proof remains open |
| R41 | Accepted icons: Doing play; Review git-pull-request; Ask message-circle-question; Failed circle-x; Ready circle; Blocked link; Later clock; Held circle-pause. Confirmed merged PR uses git-merge. | visual-style task-state icons; owner acceptance 2026-10-23 |
| R42 | Accepted palette: Review/awaiting merge #8250DF light/#BCA3EB dark, preserving other existing semantic colors. Waiting and confirmed merged result differ by glyph/label; color alone is not merge proof. | visual-style palette; both themes inspected |
| R43 | Same bundled Lucide static 1.17.0 vectors on Mac/Windows, source stroke 2 at 24-unit viewBox; provider marks separate. Reconcile missing catalog assets; reject unknown names instead of silent Ready-like circle fallback. | visual-style/assets; icon audit; runtime bundle gap remains |
| R44 | Spec first, preserve accepted geometry/palette/assets, review whole affected surfaces before presentation. Check wording, icons, density, navigation and retained state; report Pass/Fail/Not tested. Do not ask the owner to discover existing-rule violations. | R13; specq workflow/fidelity; existing revision review |

## Scope and evidence limits

The live review observed installed macOS builds 4.6.34-dev (several capture-specific revisions) and later 4.6.36-dev.1792731555. Source correlations use handoff checkout 99b2ae1cb3d566eef99ee328f6ac4581fa687cb7. Its exact correspondence to the running binary is unproved. The original dirty design checkout and frozen reference were preserved.

The per-acceptance audit records partial or unverified coverage of #1578 A1–A8 and #1580 A1–A8, amended by later native-only dialogue and retired Custom decisions. Do not interpret task closure, source routes or browser fixture passes as current native acceptance. Raw local captures and account/host observations stay local. The public transfer uses redacted/synthetic review data, clearly labelled.

Still unqualified: actual Windows/DPI; native drag, resize, blur, z-order, detached-window ownership; keyboard/IME/accessibility; real completed-history pagination; fresh cross-host login/account/expiry probes; unread-event deduplication/acknowledgement; question producer/parser and worker→answer→continuation→reviewer→close; same-ID delivery/recovery effects and immutable attachment transport. This list assigns proof, not a new platform-wide test project. Run relevant source/model/browser checks and affected native/adapter checks under the current contract and risk-based test policy.

## PM deliverable

1. Reconcile all R01–R44 against current source, existing open work and delivered-task acceptance. Mark implemented/partial/missing/unverified separately, with source or runtime evidence.
2. Return coverage from every row to canonical requirement, preserved destination and existing/reopened/new task. Keep controls/icons/fixtures/contract edits/checks inside their coherent user outcome.
3. Prefer the existing outcome groups: find work/return to it; understand/control the environment; act on requests/failures. These are grouping guidance, not three pre-filed implementation tickets or authority to rewrite scheduling.
4. Create only necessary held tasks with observable acceptance, exact reference hashes, scope, real dependencies and relevant proof. Resolve only material open policies (notably P-V1 fallback/Undo) through bounded asks before dependent implementation.
5. Link the plan and task coverage back to the handoff issue and report to the owner. No implementation/start, deployment, grant, capacity/model change or service restart is included.

Preserve O1–O3, native-only dialogue and retired Custom. Do not reopen accepted icon colors, overlay filter placement, fixed state order, global watchlist, single workspace entry, shared scroll or separate Settings window. Existing unsupported backend work remains explicit; this handoff does not create a new scheduler, gateway, auth store or conversation engine.
