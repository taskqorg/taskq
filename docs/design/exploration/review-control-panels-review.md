# Connected implementation-review candidate

Status: review draft; no blanket owner acceptance, runtime implementation or deployment. The rejected isolated icon-entry comparison is withdrawn as a visual reference.

Owner accepts the displayed icon mapping and colors on 2026-10-23. The [final correction handoff](../review-handoff.md) traces every remark and remaining qualification; earlier proposal/awaiting-approval wording below describes its historical revision. This acceptance covers the icon set/colors, not blanket native or backend acceptance.

## Worker versions, account and sign-in state

Owner adds version/account/login/expiry/freshness control per worker runtime; canonical requirement is [Worker/runtime sign-in control](../menu-product.md#workerruntime-version-and-sign-in-control). Updated twelve-screen sheet SHA-256 `551e1d8d0163d8c03662abc9a994b1254e61f1d9b1fa9a3bcff699b5c78198ed`; focused Agents preview SHA-256 `c01939f7215e578594e78524b2124793b2090431573dabfe589e4d1aba6c4b2c`, inline `worker-login-preview.html`. Geometry remains a review candidate; runtime code is unchanged.

Source inspection shows structured basic login/account/kind/refresh/rejection observations before tray text flattening (`runtime.rs:2149`, `:2257`, `tray.rs:1651` at the inspected worktree HEAD). No expiry is emitted there. Codex “ChatGPT” is a login method, so the widget keeps exact account unknown. Existing captured Claude account and versions are used, with sign-in explicitly labelled reported at capture and present validity unverified; no auth check was run this turn.

| Check | Observation | Verdict |
|---|---|---|
| Version/account | AgentQ host worker version is distinct from Claude/Codex client versions. Claude account is visible; Codex/Codex app account unknown and ChatGPT method separate. | Pass for captured values |
| Expiry | All three rows show Expiry unknown; detail says not supplied. Refresh is not relabelled expiry. Remote worker login/version/account remain unknown. | Pass |
| Detail | Login detail and Other workers disclose without nested scrolling. Access permission state remains separate from sign-in. Check/Sign in controls remain unavailable in the snapshot. | Pass |
| Layout | Focused Agents inspected at 736px/18px and 360px/18px; updated twelve-route sheet inspected at 360px/20px. Account and state wrap without horizontal overflow. Three captures in `worker-login-qa/`. | Pass for exercised views |
| Validation | Syntax, method/account assertions, final console, root spec budget and immutable baseline checks pass. | Prototype only |
| Gaps | Actual expiry/reauth/error transitions, fresh cross-host probes, expiry metadata adapters, auth notifications, native sign-in and secret-safe production transport are not implemented/qualified by this mock. No credentials, raw auth files or tokens were read. | Not tested natively |

## Saved-view deletion: active/inactive removal and Undo

P-V1 is a review proposal completing MP11, not a new owner-approved policy. Candidate SHA-256 `c1ea42dba8c0bfd0e5e023dcd7e2a5746028e63ad9a4cf359c54bde3c8e658b5`, inline `saved-view-deletion.html`. The existing Saved views chooser opens for this review; `My focus` is explicitly labelled demo data. The twelve-route Settings sheet is unchanged.

| Check | Observation | Verdict |
|---|---|---|
| Discoverability | Custom views have a directly visible labelled Delete control beside their shortcut star; built-in All tasks/Attention/TaskQ have none. A guard rejects built-in deletion. Star means unpin shortcut, not delete. | Pass in prototype |
| Active deletion | With My focus working query `drafts` and All tasks working query `icons`, deletion restores All tasks/`icons` and leaves pins 104/107 and the conversation draft intact. The chooser stays open with a named deletion receipt and Undo. | Pass |
| Restore after reload | Browser reload retains deletion and Undo; Undo restores My focus, its saved criteria, unsaved query `drafts`, shortcut state and original list position. | Pass for fixture persistence |
| Inactive deletion | Deleting/restoring My focus while All tasks/`icons` is active preserves that view/query. | Pass |
| Later edits | After active deletion, changing fallback query to `filters` then Undo restores the preset/shortcut without switching away from All tasks/`filters`. | Pass |
| Density and names | Inspected desktop and 360px/20px; a 40-character unbroken view name wraps in the chooser/current-view strip, with star/delete controls reachable and no horizontal overflow. Receipts do not duplicate between open chooser and dashboard. | Pass for inspected cases |
| Verification | Syntax, built-in guard assertions and final browser console passed; no warnings/errors. Four captures in `review-2026-10-23/saved-view-qa/`. | Prototype only |
| Limits | Latest deletion only, no expiry timer, persisted preference snapshot. Native write failure/concurrent edits, OS focus/keyboard and true restart durability remain unqualified. No actual saved view or task was removed. | Not tested natively |

The fallback and Undo rules remain for owner agreement. Older deletion behavior (immediate removal without recovery, custom detection by ID prefix) is superseded in this candidate.

## Settings sheet: twelve routes using captured native data

Review-only proposal, SHA-256 `8ebe118f12c0e3e120f9b902d9c86b8137a887cf9c37ae222d9d8f296f4b79ac`; source `settings-review.html`, inline `settings-overview.html`. Twelve route snapshots are shown side by side; this is a comparison sheet for the existing Settings singleton, not a proposal for twelve simultaneously opened native windows. Previous accepted dashboard views remain unchanged.

| Gate | Observation | Verdict |
|---|---|---|
| MP14 / D-A11 / W7–W10 | Actual captured host names, unknown runtime counts, separate occupied-slot/cap values, CPU sample omitted and available memory sample omitted; capture ages remain labelled. No live-update claim or inferred zeros. | Pass for snapshot |
| MP15 / W8 / W13 | Seven project rows, seven delivery policies, one observed host-service PID with six separate recorded incidents, grouped equal version reports; secondary prose/source detail disclosed. | Pass for captured scope |
| I1–I3 | Summary preview reuses existing numeral placement and shows observed board work 2 / answers 0 / ready 0 with the six captured event bins. It is not a native tray raster qualification. | Pass as design preview only |
| W15 | Selecting taskq Review reveals actual #1654/title in place; exact board links preserved. Five observed open task identities underlie the project drill-down. | Pass |
| Shared controls/state | Question mode changes independently; Closed remains Banner. Expanded host reconciliation survives rerender/font changes. All preferences remain local to the review; no save on load. | Pass for exercised controls |
| Layout / D-A4 | Inspected all 12 screens at 1024px/dark/18px; inspected 360px and 320px/light/20px. No horizontal overflow or internal scroll owners; narrow sheet stacks routes. Counter marks remain visible; host line wraps only where narrow space requires it. | Pass for inspected widths |
| Missing evidence | Access readback remains unconfirmed, Recovery read unconfirmed, other recovery targets not read, completed-task records not loaded, update metadata not checked. No fabricated healthy states/results or native reply support. | Explicit gaps |
| Final checks | JavaScript syntax, evidence-count assertions and browser console passed. Frozen control-panels baseline is unchanged. Close/effectful controls stay unavailable; no native actions were exercised by this sheet. | Prototype only |

Six valid viewport captures in `review-2026-10-23/settings-sheet-qa/` cover the top four routes, middle four routes, bottom four routes, narrow Projects, Appearance and Notifications/Activity. Full-document screenshot clipping was rejected; viewport captures were inspected instead. Light/dark, disclosure and row-state behavior do not qualify actual OS window management, continuous refresh or native accessibility/IME.

## Current revision: accepted views, one workspace entry, no menu Enlarge

Owner confirms the current views and requests one Conversations/workspace menu icon and no Enlarge button in the task menu. MP12/P-W3 and the active session were updated first. Candidate SHA-256 `8eaf818ad53cbf9ac29ff846defec40a38f50050cd2b64a3ebd212f5d1487c4f`, inline `single-workspace-entry.html`.

Prototype verification: one communication entry, no separate Inbox entry in the menu header, no menu Enlarge, one corner resize control, and exactly one menu content scroll owner. A single click on the entry opens Inbox when its sample events need action; the task query `filters` remains unchanged and the badge stays 3 (opening alone does not acknowledge events). Route assertions cover Inbox, Conversations-only and no-event restoration. Settings/Conversations still retain their own enlargement controls. Desktop screenshots `single-entry-qa/01-header.png` and `02-pending-inbox.png` were visually inspected; syntax/assertions and final console passed. Live unread adapters/deduplication are not implemented by these synthetic fixtures.

A fresh native installed-application review is now recorded separately in [live settings review](live-settings-review.md), with ten selected captures, observed data/form gaps, route coverage and three proposed correction outcomes. It does not certify every acceptance criterion or authorize runtime changes.

## Current revision: compact host counters and one content scroll

Candidate SHA-256 `3fb6a0d83d23a105f888a97b3d87a634752751ec8e811f91ba041ab98aa886eb`; inline copy `compact-host-review.html`. Owner explicitly keeps separate Claude/Codex counts visible beside each host in the collapsed single-line overview. Only resources and freshness expand. The owner rejects independent scroll panels: this supersedes the preceding reserved/stationary watch area layout. Global pins remain first and independent of filters; they scroll with hosts, filters and ordinary results. Window controls/version footer stay outside the shared content viewport.

| Check | Observed | Verdict |
|---|---|---|
| Collapsed overview | Both named hosts and both runtime marks/counts share one line at desktop and 360px/20px. Stale win remains unknown, never zero. | Pass for fixture |
| Expansion | Click opens CPU/memory/freshness; native Return closes via the focused button. `aria-expanded` and detail visibility agree. | Pass |
| Scroll ownership | Collapsed/expanded menu has exactly one scrollable descendant: `rq-main`. Page Down moves this viewport to 154.5px. Pins/filter/task containers have no independent overflow. | Pass |
| Context | Expansion with query `no-match` retains that query, pins 104/107 and zero ordinary results. | Pass |
| Visual | Inspected desktop collapsed/expanded and narrow 360px/20px; counters fit, no horizontal overflow. Original palette/provider marks preserved. | Pass for inspected fixtures |
| Validation boundary | Syntax and existing assertions pass; final browser console has no warnings/errors. Counts are samples; native sizing/scroll behavior and live telemetry remain unqualified. | Prototype only |

Three captures in `review-2026-10-23/host-overview-qa/`. Prior revision tables below are historical where they describe separate scroll owners or stationary pins. Runtime, task dispatch and deployment remain unchanged.

## Current revision: accepted global task watchlist

Candidate SHA-256 `d58af202f0560d651f7ae83a05f690d2c38714df2f6332be6c756ac86ec1f621`; inline copy `global-pin-review.html`. Owner accepts separate Settings windows and replaces the earlier per-view/filter-scoped pin proposal with a global, always-visible watchlist. Canonical P-W1/P-W2 and the active session were updated before the prototype. Prior revision tables below are historical where they describe per-view pins or hidden-pin counts.

| Check | Observed | Verdict |
|---|---|---|
| Global scope | Pins 104/107 persist through All tasks, Attention, TaskQ, project scope, a no-match query and Completed history. A no-match query leaves zero filtered rows/All 0 while both global pins remain. | Pass in prototype |
| Compact Pin | Pinning 105 closes details and adds one ordinary compact row to the watch area. Three pins plus remaining results have 8 rows and 8 unique task IDs. | Pass |
| Unpin | Unpinning 105 outside the active project/search removes it from view; no filter is cleared or broadened. | Pass |
| Reserved layout | Watch top stays at 195.796875px while the ordinary list scrolls 22.5px. The watch section occupies layout space outside the results viewport. | Pass for exercised scroll |
| Persistence/migration | Reload after rendered-anchor readiness restores 104/107 with details closed. Assertions merge legacy view pins in insertion order without duplicates. Reset and view switching do not modify the global references. | Pass for fixtures |
| Narrow / type | Light 360px / 20px: watch bottom 605.984375px precedes filters at 609.984375px; ordinary list retains 153px and no horizontal overflow. Narrow demo height was increased to 820px to keep ordinary rows readable; this is not a production native sizing default. | Pass for inspected fixture |
| Existing windows | Settings and Conversations remain separate and retain their common chrome. Font changes and Settings close leave the global watchlist intact. | Pass in prototype |
| Source/data boundary | Persist references/preferences only. Real canonical references, completed/unavailable pinned records, native window sizing and large-watchlist overflow still require implementation/platform evidence. The static fixture contains eight known tasks. | Not tested beyond fixtures |

Three captures: `review-2026-10-23/global-pin-qa/` contains the no-match filter with visible global pins, light/narrow large type, and normal watchlist. Final console was free of warnings/errors; JavaScript syntax and pin partition/migration assertions passed. Runtime code, board state and deployment are unchanged.

## Latest revision: task pins and owned windows

Candidate SHA-256: `06a56da23f5ce418abe793a569601b18b3f8739a0ab7f4422bb6f8fa74af174c`. Prior connected-review observations below describe the preceding revision; this section owns the changed pin/window scope. Product placement and pin/filter semantics remain [P-W1–P-W3](../menu-product.md#window-and-task-pin-review-proposal) proposals for owner review.

Source findings: `src/task-dashboard.js` popup-pin latches the detail overlay; view-pin pins a saved-view shortcut. Neither promotes a task. `src/tray.rs` workspace-window pin calls `set_always_on_top`; the workspace constructor does not explicitly disable native decorations. This source inspection does not establish the exact installed binary or native behavior.

| Rule | Observation | Verdict |
|---|---|---|
| P-W1 | Fixture pins 104/107 occupy a normal-flow section above other tasks. Pinning 105 yields 3 pins, 8 total rows and 8 unique identities; unpin restores 105 as the first ordinary task. | Pass in prototype |
| P-W1 scope | Search `drafts` leaves 1 matching task and reports 2 hidden pins, without inserting excluded rows. Reset does not clear pins. Task action visibly says Pin to top / Unpin task; an old detail-latch control is not presented as task pinning. | Pass in prototype |
| P-W2 | Settings/Appearance opens in its own window container while task search remains `filter`. Closing returns the existing workspace; reopening restores Appearance. Draft text survives Settings opening/closing. | Pass in prototype |
| P-W2 routes | Re-exercised Work mode, Projects, Agents/access, Resources, Services, Delivery, Appearance, Notifications, Activity, Recovery, Diagnostics and About inside the Settings window. Completed remains the task-discovery view. | Pass for route coverage |
| P-W3 | Menu, Settings and Conversations call one windowHeader renderer and share drag hit regions, Close, enlarge/restore, separate Always on top and corner resizing. Only the transient menu has Keep open. Inspected full images and a header crop confirming all control glyphs. | Pass for rendered proposal |
| D-A4 / C-A10 | Inspected 1024px dark/16px and 360px light/20px. Visible controls stay within the 328px root; pinned rows and the separate Settings window do not overlap. Narrow preview stacks owned windows for review. | Pass for inspected fixtures |
| Checks | node --check, pin partition assertions and final browser console checks passed. Existing independent task-filter/draft behavior retained. | Pass |
| Native | Real undecorated windows, OS drag/hit-testing, task switching, z-order, placement/accessibility and persistent canonical task-reference migration are not implemented or qualified by the simulation. | Not tested |

Three QA captures are in `review-2026-10-23/window-pin-qa/`: Settings with task pins, matching Conversations chrome and light/narrow layout. The inline candidate is `window-pin-review.html`. In the review stage, Settings and Conversations take turns in the right-hand foreground slot; their state remains separate. This does not claim native concurrent window management. Demo pins are fixture data, not a proposed default. No runtime source or board state changed.

## Baseline and intentional changes

Frozen visual baseline: `control-panels.html`, SHA-256 `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`. Candidate: `review-control-panels.html`, SHA-256 `4568ab09c8fd8b039b5a0fabd900ae0531189c5b3af39fe48d66deb965197a35`. Continue the original surface/type/palette and bundled provider assets; Lucide remains the action family. The candidate is editable, not generated by the older workspace builder.

Authority: taskq.md Product/R13; visual-style revision contract; menu-product MP11–MP15; ai-workspace review input contract. These were updated before authoring the candidate. AGENTS now links the same revision-fidelity gate instead of adding a second rule set. Existing later native-only dialogue and retired Custom decisions override their archived examples.

Intentional scope: quiet row dividers; stronger opaque popovers; click-only task anchors; runtime/remote marks before timing; named-host/runtime counters and CPU/memory meters; one-click Home/Reset; custom-view Save/delete; completed-history browsing; unified Appearance; readable operational route inventory; separate pin/topmost; version-only footer/grip; one shared composer for supported text/file entry; independent drafts, New, Archive/Undo and explicit deletion scope. Full dialogue remains native. No one-versus-two-icon choice is requested.

## Pre-presentation review

| Rule | Observation / evidence | Verdict |
|---|---|---|
| F1 / D-A9 | Original palette and source provider assets reused. Shape/size is the existing forge/issue anchor, not a new boxed status column. Lucide 16px with original stroke weight. | Pass |
| Revision contrast | Task divider is Text at 8% over Surface; control edges remain separate. Raised panel uses #FFFFFF/#323B45 and #8996A2 edge. Inspected dark desktop question and light/narrow Appearance. | Pass |
| MP12 / M13 | Actions precede titles; Home/Back, Pin, independent filled-foreground-square Topmost, Close and version-only footer/grip present. Native topmost/placement not exercised. | Pass in prototype / native not tested |
| MP11 / O3 | Changed search survives Settings → Appearance → Home; All scopes to 1 before state selection; Reset restores 8 rows. Project filter yields 4; overlay stays open and list does not reflow. Save-copy and custom-view deletion exercised. | Pass |
| O2 / D-A1 | Comparator self-check produces 105,107,108,103; grouped view uses the same comparator inside each project. Metadata is followed by full-width title; no task ellipsis/extra action column. | Pass |
| MP13 / keyboard | Focus/Tab leaves the popover closed; Space opens it. There are no hover/focus opening handlers. Source-numbered option 1 yields a clearly marked sample accepted receipt. | Pass |
| MP14 / overview | Named mac/win groups show runtime identity; stale win has dashes and no fabricated meters. CPU and available-memory meters have units/tooltips; details distinguish process counts, policy and board state. All numbers are fixtures. | Pass in prototype / live parity not tested |
| MP14 / history | Period/project/search/outcome controls exist. Seven days gives 3 fixture rows; 30 days gives 4. Cancelled filter gives only not-planned record; unknown closure is separate. Result/history panel opens from a record. | Pass in prototype / board pagination not tested |
| MP15 / routes | Exercised all 13 Settings destinations: Work mode, Projects, Completed, Agents/access, Resources, Services, Delivery, Appearance, Notifications, Activity, Recovery, Diagnostics, About. Each has a nonempty named surface; native actions explain unavailability. Host service appears once per host. | Pass for route coverage / operational adapters not tested |
| C3 / D-A10 | One composer renderer serves draft and free-text answer. Same context, text, files, validation and send arrangement. Task file-answer attachment is disabled with an adapter reason, not advertised as working. | Pass for presentation |
| C2/C7 / continuity | Independent draft survives switch and Archive → Undo. New selects a fresh draft and focuses text. Native launch does not run; attempting it leaves an explicit preview notice and retains the draft. | Pass in prototype |
| D-A4 / C-A10 | Inspected 1024px dark at 16px and 360px light at 20px; no horizontal control overflow. Fixed menu/workspace content scroll with reachable footer/composer. Inline SVG display initially split timing/headings; fixed and re-inspected. | Pass for inspected fixtures |
| JavaScript | `node --check` passed; embedded comparator/filter/unknown assertions passed; final browser console had no error/warning entries. | Pass |
| Native / transport | Actual native drag, resizing, session raising, permissions, lifecycle actions, notification delivery, file bytes/limits and real question producer/parser/dispatch/receipt coverage remain unqualified. | Not tested |

Browser captures: review-2026-10-23/connected-qa/{01-dashboard-workspace,02-question,03-completed,04-light-20-narrow}.png. They are a four-image prototype set, separate from the nine retained native captures. Main live findings remain in review-1573.md; prototype passes do not close them. Dates, IDs, resource values and receipts are illustrative; recognizable real issue/PR references do not make fixture metadata live.

## Icon and Review color audit

The owner requests one consistent icon set and a purple accent for Review/awaiting merge. Semantic mapping is a review proposal in [visual-style](../visual-style.md#task-state-icons); palette intent is owner-requested. [Simultaneous comparison](icon-review.html) shows all eight states and the distinct Awaiting merge/Merged PR meanings. The frozen baseline's hash remains `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`.

Inspected source: handoff worktree `99b2ae1cb3d566eef99ee328f6ac4581fa687cb7`, `src/project-blocks.js`, `src/ui-icons.js`, `src/ui-icons.json` and `src/project-blocks.LICENSE`. The current installed binary's exact source identity is not established. Prior broad icon-fidelity Pass claims above do not qualify the conflicting mappings identified here.

| Meaning | Connected draft before review | Inspected implementation / unified proposal |
|---|---|---|
| Doing | loader | play |
| Review | scan-eye, mint | git-pull-request; proposed purple accent |
| Ask | message-circle-question | same |
| Failed | circle-x | same |
| Ready | circle | same |
| Blocked | git-pull-request-arrow | link; dependency, never merge |
| Later | clock-3 | clock |
| Held | pause | circle-pause |

The Settings sheet already used play/git-pull-request/link/clock, but Held used pause. Both editable candidates now share the eight-state glyphs; counters, task rows and Settings project summaries use the purple Review token. Review and awaiting merge remain waiting states; git-merge identifies merge policy/actions or an explicitly confirmed Merged PR result, never Blocked. There is no new TaskQ merge state. Closed/history emphasis and board-label colors are unchanged.

Asset findings: the inspected runtime combines 49 bundled assets, using Lucide static 1.17.0 plus provider/forge marks. The old prototypes relied on host-renderer Lucide and stroke width 1.7 instead of the bundle's source width 2. Current state glyphs now reuse exact bundled SVG bytes; the review sheet also reuses the bundled common controls/marks. Merged uses the upstream 1.17.0 git-merge vector, with its embedded license notice; this asset is still absent from runtime. Runtime `icon(name)` silently substitutes circle for missing names, which can falsely resemble Ready. Reject missing catalog names at validation/build time instead of accepting a valid-looking substitute.

Other prototype-only names absent from the inspected bundle include archive, bookmark, bookmark-plus, chart-no-axes-column, chevron-down, clock-alert, file-text, gauge, history, house, list, paperclip, pencil, star, trash-2, type, undo-2, badge-check, check-check, circle-help, log-in, refresh-cw, rotate-cw, shield-alert and user-round. They are real Lucide names, not necessarily invalid glyphs; source/version/license coverage must be reconciled before implementation. Do not silently replace them with another semantic icon. Topmost's filled foreground-square composite and corner grip remain deliberate shared controls, not task states. Provider marks remain separate from state symbols.

| Rule | Observation / evidence | Verdict |
|---|---|---|
| State consistency | Five drifted draft states identified; Settings Held reconciled. Eight proposed glyphs are identical across current mapping/row/filter renderers; missing-name guard in the audit sheet throws rather than substitutes. | Pass in prototype/source inspection |
| Review accent | Light #8250DF follows Primer done; dark #BCA3EB is a TaskQ adaptation. Contrast against TaskQ base surfaces computes 4.87:1 / 6.51:1. These checks do not qualify every selected/hover/OS accessibility state. | Pass for base surfaces |
| Visual review | Inspected complete state comparison in dark and light, expanded controls/brand inventory, connected dark dashboard/workspace and Settings Projects. Review purple appears in counters and rows; Blocked is a neutral link. Source stroke width 2 is intentional. | Pass for inspected candidates |
| Narrow sheet | At 360 px viewport, iframe client/scroll widths both 328 px; all eight state pairs and PR labels remain visible without horizontal overflow. | Pass for inspected comparison |
| Source/runtime boundary | Exact byte reuse covers current state symbols and the audit's bundled inventory. Remaining prototype-only assets need pinned catalog reconciliation; build-time catalog rejection and native Mac/Windows rendering/DPI are not implemented or qualified. | Not tested / implementation gap |

Evidence: `review-2026-10-23/icon-qa/` has five captures (light catalog, connected dark, Settings projects, narrow light, filter editor). The filter editor's Review icon resolves to `rgb(188, 163, 235)` and the exact bundled mask. JavaScript syntax, exact state-asset byte comparison, root contract bounds and frozen-reference hash checks passed; all three browser consoles had no warning/error entries. SHA-256: connected `268728dafd8a9683160b14d2736da4b99cdcbcace4b1e34648a8bd60b4f410d7`; Settings `886cadec0e288350dba10dad9e44444f48f9e2aba6aede68a8292da1fdb0e693`; icon sheet `42b963daeefefaf783070579c596a9e7365b44754becef0c8dd2f9e7fbcdfa6b`. No runtime, board, login, service, commit, push or deployment changed.

## Remaining implementation work

Connect authoritative board history/pagination and real host/runtime snapshots; preserve unknown semantics. Consolidate runtime request composers and retire the duplicate legacy route. Qualify question forms end to end without inventing answer/file transport. Implement the shared tokens/components in the actual app, then repeat native acceptance on the exact delivered binary. Summary selection and native tray raster/DPI still require the real renderer; this candidate reuses the compact Q asset and does not replace that renderer with a lookalike. No runtime source, service, policy, board task, commit, push, PM message or deployment changed in this review revision.
