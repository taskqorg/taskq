# Approved fix plan from the owner's custom answer

The owner approved the six briefs and directed filing in order in
[answer 6311797833](https://github.com/taskqorg/taskq/issues/1443#issuecomment-6311797833).
This plan records that decision; D must amend the owning specification before UI
implementation. Running behavior is unchanged (R13, § 7.5).
The [custom answer](https://github.com/taskqorg/taskq/issues/1443#issuecomment-6311252518)
replaces the old three options and H/T/S/L/U plan. The
[interaction addition](https://github.com/taskqorg/taskq/issues/1443#issuecomment-6311273212)
is included. The approval and archive override supersede the earlier ask options.
No UI changed in this review; the filed-task receipts are recorded below.

## Owner requirements and approved placement

The seven requirements, placement and message meanings below are the approved
briefs. The archive/deletion override in answer 6311797833 wins over the earlier
terminal-only proposal and the existing C7 restriction; D updates C7 explicitly.

| Point | Required outcome | Approved placement / acceptance owner |
|---|---|---|
| 1 | One standard navigation bar on every screen, with Back built in; meaningful breadcrumbs | N: fixed Back/title/actions pattern in both windows; no separate Back row. At a window root, no misleading cross-window Back. Nested project/settings paths may show breadcrumbs; a flat conversation list need not. |
| 2 | No dialogue, answer cards or inline confirmations in the menu | W moves conversations and task answers to the workspace; O moves operation reviews there. Menu clicks open the exact workspace context and never submit or confirm. |
| 3 | Main menu contains panels with useful live figures, then the task list | P: compact host/runtime executing-agent panels, compact work controls and setting links, then tasks. Slots, reservations and Doing/Review task totals retain separate meanings. |
| 4 | Place other content according to usability | The route review below assigns every existing route family. D records the accepted destinations before implementation. |
| 5 | Messages icon with a count opens a message list; archive, show archived and clear archived | M: workspace message list and conversation history; archive any conversation and permanently delete one or all archived conversations under the approved boundary below. |
| 6 | Review the whole tray/workspace against points 1–5 | This source/route review covers current placement and gaps. Every implementing task repeats its affected flow; final combined acceptance uses the whole route/state matrix. |
| 7 | The same control follows the same interaction rule everywhere | N provides shared behavior and tokens; #1455 owns the existing toggle-outline repair. The inventory below identifies every differing control family found in the inspected sources. |

Approved message meaning: the badge counts pending message entries, not agent
processes, slots, tasks or all historical replies. Entries are current task
questions, operator clarification/proposal cards and unread terminal replies.
Deduplicate task cards by board/task/revision and operator messages by conversation,
request and answer revision. Reading removes unread status; an unanswered card
remains pending. Drafts and routine running updates add no badge entries. Incomplete
or stale collection shows unknown with a reason, not zero. D must record these
meanings and source identities before M implements them (R1, R6, R12).

A “session” in the archive controls means a TaskQ conversation, not a live
worker/reviewer or an unrelated native chat. **Any conversation can be archived**,
including active or unresolved conversations; it leaves the ordinary list. The
archived list can be opened, and one conversation or all archived conversations
can be deleted permanently, with no recovery. This replaces the earlier
terminal-only archive and recoverable list-only alternatives.

Deletion removes only the archived conversations and their attachments. Board
tasks, source files and live work remain: archiving/deleting does not stop an
agent, end an attempt, remove an executable request/receipt needed by live work,
or change task truth. D must define the conversation-versus-execution ownership
boundary before M implements deletion; do not retain recoverable conversation
copies as a substitute for that boundary. Source attachments remain untouched;
only conversation-owned attachment copies are removed. Validate containment and
ownership, read back deletion, and report a partial/unknown deletion honestly.
Window close, archive, cancellation, command-history clearing and deletion remain
distinct. Existing confirmation/review authority stays unless D explicitly
amends it; the owner has not requested a new undo, trash or recovery feature.

## Whole-surface usability review

These are source observations against product source `8669a08ec2435e830301bf7307fb37f477c87e6d`,
plus the source-labelled browser evidence in [the findings](README.md). They are
not native usability acceptance. The owner's disabled tray Send symptom is a
reported failure; its native cause has not been reproduced. Moving the composer
alone does not establish a fix for Send.

| Existing route / control family | Observation and user effect | Approved destination / retained behavior |
|---|---|---|
| Home: host/work controls, Scope, Attention, agents, AI and Projects | Tasks follow several sections; empty Attention still occupies space (G06); AI dialogue is in the menu | P puts compact useful panels before the task list. W removes the menu composer/transcript. M owns the messages entry. Empty summaries disappear; faults and unknowns remain visible. |
| Projects, project detail, task filters, All tasks | Project/task identity repeats (G05); task questions expand inline; state and All tasks selection differ | Keep task browsing in the menu, one identity per context and full titles; answer opens W. Preserve OR filters, global All tasks scope, zeros/unknowns, independent Board/issue/native-session links. #1455 repairs toggle selection. |
| Attention, failed tasks, operational faults | A fault summary and an owner conversation have different purposes | P keeps compact actionable fault/status panels; questions and operator diagnosis open W/M. Do not silently retry failed tasks or count faults as chat messages. |
| Settings, Appearance, cadence/history, Notifications | Settings are separate routes but title/Back occupy content (G04/G01); selected buttons differ by group | N supplies shared navigation and controls. P groups setting links by purpose; settings remain dedicated screens, with project/host scope visible. Archive is not cadence history or last-20 command history. |
| Agents, host/runtime detail and Work sessions | Existing executing-agent snapshot is separate from capacity; some workspace destinations are not implemented | P shows actual executing workers/reviewers per host/runtime. Details open an existing supported destination; unsupported destinations explain availability. Do not fabricate native links or expand this plan into a new agent-settings backend. |
| Working mode, service, limits/resources, stop/recovery/update | Reviews and receipts currently appear in menu pages/cards | Compact status and launchers remain panels; O opens exact targeted review in workspace. Preserve pause/stop/emergency distinctions, grants, revision fences and readback. Simple settings with no review retain their named direct action/readback. |
| Add project/setup, attachment picker, maintenance | Setup includes external effects and native selection, not just navigation | O presents setup/maintenance review in workspace; retain § 6 exact-effects approval and native owner picker. W owns attachment/draft behavior. No setup effect on opening a screen. |
| Workspace list, conversation, composer and history | Conversation selection uses a different visual cue; narrow navigation is ad hoc; no archive controls in current JS/backend | N standardizes navigation. W retains IME, drafts, source card identity, one in-flight request and same-ID reconciliation. M adds list/archive flow with C7 ownership. |
| Workspace unavailable/loading/update/error/partial states | Refresh can lose focus; no routine status/live region (G08) | W preserves focused controls/caret through updates, announces status politely, explains unavailable Send and recovery without weakening duplicate/unknown guards. |
| Both windows: close/pin/top/resize, links and keyboard exits | Header clipping, small targets and weak type/contrast are measured (G02/G03/G04/G07) | N applies shared target/type/contrast/focus rules and bounded overflow. Keep window actions distinct from route Back and emergency stop. Native picker/window sizing still requires native evidence. |

## Interaction differences to remove

Source inventory: `src/tray-app.js`, `src/project-blocks.js`, `src/tray.html`,
`src/workspace.js`, `src/workspace.html`, `src/resource-controls.js/css` and
`src/answer-card.js/css`. This is every differing family identified in that bounded
source review; dynamically generated native controls and all keyboard paths are
not fully qualified. This table is the repair checklist, not a claim of a pass.

| Family / differing places | Current difference | Planned common rule / owner |
|---|---|---|
| State chips versus All tasks | State selection gets outline; All tasks omits pressed state | Existing #1455 owns pressed state and selected outline for all toggle/filter groups. N consumes and verifies it rather than duplicating its task. |
| Font size, macOS view mode, history period, host/project notification modes, pin and keep-on-top | All use pressed buttons, but generic fill/border differs from state-chip outline | #1455 includes the group sweep; N gives equivalent toggles one selected/focus treatment and verifies enabled/disabled/keyboard states. Platform-only groups remain platform-qualified. |
| Conversation row versus selected filter/navigation entry | Conversation uses aria-current and a border; filters use aria-pressed and an outline | N/W share a recognizable selected treatment while keeping correct semantics: current destination is not a toggle. Do not replace aria-current with aria-pressed everywhere. |
| Tray Back/content title versus workspace narrow-list/Back controls | Different placement, labels and return behavior; invoker not restored | N uses one nav pattern, route history and invoker/fallback focus with scroll/filter restoration. Workspace remains its own root. |
| Send button versus Enter, menu versus workspace composer | Different guards and pending behavior; owner reports disabled Send in menu | W removes menu composition and uses one submit guard in workspace. Valid ready text enables Send and Enter; disabled states explain the reason. Preserve IME, Shift+Enter, duplicate refusal and uncertain-delivery reconciliation. |
| Task answer, operator proposal and resource/setup review | Reviews/drafts/receipts are spread through menu and workspace | W/O use workspace context and common pending/stale/partial/accepted feedback; ordinary answer actions keep exact option IDs and do not gain an unnecessary confirmation. |
| Refresh, Back and changed snapshot | Some updates retain draft/caret; Refresh loses focus and Back focuses H1 | N/W restore active/invoking controls or a named fallback; status announcements do not steal focus. |
| Buttons, linked task/Board/session targets, checkbox labels, selects and resize | Effective dimensions and keyboard reach differ (G02/G03) | N measures effective labelled targets, both dimensions and overlap at fine/coarse input; reuse native checkbox/select semantics. Links stay independent; action buttons do not acquire selected state. |
| Archive versus close/cancel/clear history | Archive/clear controls are absent, so no implemented behavior can pass | M allows any conversation to be archived. One/all permanent deletion uses the same conversation/attachment ownership boundary; live execution stays intact even for active/unresolved conversations. |

## Six approved follow-up tasks

These six briefs are approved for filing. D is first. Each row is one user-visible
outcome; checks span its layers rather than becoming separate probe tasks.
No parallel writers edit the same shell/shared files. If a row cannot stay one
reviewable diff after the foundation handoff, ask about a scenario split before code (R2).

[#1440](https://github.com/taskqorg/taskq/issues/1440) is now closed: the owner-selected
Preact + Signals + Wouter decision was merged in
[PR #1450](https://github.com/taskqorg/taskq/pull/1450), merge
`204593a0f7c12261b44aa00953f3063736389ea5`. The
[foundation migration](https://github.com/taskqorg/taskq/blob/204593a0f7c12261b44aa00953f3063736389ea5/docs/design/app-foundation.md#migration-task-list-a4)
M2–M5 are listed as unfiled in that specification; this review does not establish
their implementation readiness. Closing #1440 does not prove it. Use its M2 calibration/budget approval and M3 workspace,
then M4 tray handoffs; do not create a second store/router or silently approve
those unfiled implementation tasks through this plan. N is filed held until the
corresponding M3/M4 handoffs exist and are verified; its descendants wait on N.
D depends on this research task's close so the reviewed plan is available first.
O consumes W's workspace context, so its dependency also serializes the shared
workspace changes. No additional foundation task is created by this approval.

| ID / outcome | Scope and real prerequisites | Observable acceptance and retained risk protection |
|---|---|---|
| D — Record the accepted menu/workspace contract | First task, documentation only: taskq.md Product, R6 and § 4 references plus docs/design/menu.md, adaptive-menu.md, answer-card.md, ai-workspace.md, ui-rules-v2.md and design index. Link foundation and counters; name amended R-numbers in result (R8/R13). | Record points 1–7, destinations and message counts; explicitly replace C7's terminal-only rule with archive-any and one/all permanent deletion without recovery. Remove conflicting inline-menu dialogue decisions and stale checkbox clauses. Keep TaskQ-specific placement separate from shared answer semantics/DashQ. R1/R9/R11 authority, active work, source option IDs and exact-revision receipts remain. No runtime claim or source change. |
| N — Navigate and operate both windows consistently | After D and corresponding #1440 M3/M4 foundation handoffs; reuse #1455 selection repair. Shared nav/control components and per-surface tokens, existing public assets remain framework-free islands. | Back/title/actions in one fixed bar, meaningful breadcrumbs and explicit roots; route/reload/scroll/filter/invoker retention. G01–G04/G07 fail before, pass after: ≤1 px Back drift, reachable Close at 320 px/200%, fine/coarse ≥32×32/44×44 nonoverlapping targets, b/1.1b type and 4.5:1 text/3:1 essential graphics or valid exceptions. Every toggle family above has consistent feedback and correct ARIA/keyboard behavior. |
| W — Answer and converse only in the workspace | After D/N and workspace foundation handoff. Move menu composer, attachments, task cards, clarification and operator proposals to exact workspace contexts; retain existing backend/asset boundaries. | No dialogue/composer/answer card remains in menu; click opens without sending. Reproduce or bound owner Send symptom; valid Send/Enter agree, invalid reason visible. Preserve drafts/IME/caret, full replies, same-ID retry, task/attempt/revision and source choices. G08 focus/live-status after-pass; loading/stale/failed/partial outcomes preserve work. |
| O — Review consequential actions in the workspace | After D/N/W and workspace handoff; consume W's shared context. Move service/resource, emergency/recovery, update and setup review/confirmation/receipt views; reuse existing operation executors. | No inline confirmation/dialogue in menu. Workspace names exact target/effects before apply; cancel/Back applies nothing. Preserve grants, § 6 setup approval, revision/operation identity, single-use confirmation and readback; unknown/partial does not become success or replay. Keep native picker and distinct emergency semantics. |
| P — Make the menu a task list with useful live panels | After D/N/W/O and tray foundation handoff so moved features have working destinations. Recompose Home/project/settings navigation, reusing existing runningAgents snapshot and shared project blocks. | Compact host/runtime executing-agent figures and work/settings launchers precede full task list. Workers/reviewers deduplicate by board/task/execution; stale/partial is unknown, slots/reservations/task totals labelled separately. G05/G06 after-pass: no repeated identity or empty summary. Preserve R6 counters/OR filters/All tasks/no policy writes, independent destinations and active fault visibility. |
| M — Read and archive messages from one entry | After D/N/W/P and workspace foundation handoff. Messages badge opens workspace list; native-owned local conversation metadata, archive/show/clear flow. Current conversations.rs exposes no archive/delete API; this is implementation work, not a claimed existing feature. | Badge/list use the specified stable identities/count and unknown handling. Open never resends; returning retains list/filter/draft. Archive any conversation; open archived list; permanently delete one or all with no recovery. Delete only conversation history/owned attachments; preserve board tasks, source files, live work and its required execution evidence; restart/readback and partial failures preserve ownership. |

Rule ownership for filing: D — R8/R13 and V37; N — V01–V06, V09–V14,
V20–V26, V34/V35; W — V04/V07/V13/V15, V27–V29/V36; O — V15/V28;
P — V08, V16–V19/V21, V30–V34; M — R1/R12 and C1/C7 of ai-workspace.
V38 applies to every task's actual-size acceptance. Overlap names retained
protection at the handoff, not competing writers or duplicated fixes.

N owns shared visual/target/navigation gaps; W owns dialogue/update accessibility;
O owns review placement; P owns menu composition; M owns message/archive lifecycle.
N includes the former T/S scope rather than dropping its risk protection. No
standalone “review everything again” implementation task is added. Before the last
UI task is accepted, review the combined tray/workspace against all seven points
and the interaction table. Fix an acceptance gap in its owning open task; do not
spawn a parallel writer for the same surface.

## Evidence and verification

The [eight screenshots](screenshots.md) are the complete review set (≤10), one per
distinct gap; image bytes/hashes are unchanged. W's removal of inline task dialogue
is visible in G05 and the menu composer in G06; O and M have source/requirement
scope rather than invented screenshots of unimplemented views. The
[304-cell text matrix](current/matrix.md) retains passing/gap observations without
per-cell PNGs. It remains pinned to its measured source, not to latest main.

For each fix, retain this named failure before, run the affected scenario after,
then the selected project gate. Final whole-flow review covers every supported
route in both themes, type sizes, narrow widths and pointer modes with a text
pass/gap table, keyboard exits, error/unknown/retry and preservation of active work.
Keep at most ten distinct-gap images across the review set; owner actual-size
acceptance and native WKWebView/WebView2 qualification remain separate (R12).
GitHub publication requires green merge-result merge_group CI; private fallback
uses exact-SHA CI. No CI pass, deployment or native qualification is claimed here.

Before this revision, commit `46ba3540391243f4377273f9c0da4aec78e76680`
still proposed five H/T/S/L/U briefs and omitted the custom answer. After this
revision, seven owner points map to six briefs, the full route/interaction
inventory. The owner then approved the briefs and replaced the archive restriction
with archive-any and permanent one/all deletion, preserving live work. Structural checks pass for all seven
points, six listed task dependencies, 38 retained verdicts, two 304-row text
matrices, local links/anchors, eight unchanged PNG hashes and evidence-only scope;
`git diff --check` passes. These checks validate the plan, not the future UI.

This continuation records the approval/archive override and six read-back TaskQ creates; repository edits remain documentation only. It does
not rerun unchanged browser/native code or weaken an assertion. The earlier full
canonical gate failure (427 passed, 21 failed, 36 ignored) remains in
[checks.md](checks.md); sandbox-blocked host-policy checks and other unqualified
failures remain distinct. D must amend the specification before any implementation
changes those accepted placements; this evidence report does not amend running policy.

The owner answer is the § 7.5 approval; no second plan approval is required.
Foundation migration still follows its separate #1440 handoff. #1455 retains
ownership of the toggle-selection repair. D records the archive override in
taskq.md/docs before M changes code; this research task only files that outcome.

## Filed tasks

All six creates were issued once through TaskQ after owner answer 6311797833.
Each issue title, all eight acceptance items, dependencies and routing labels
were read back. The configured project has no create-claim issue, so ordinary
(non-idempotent) add was used; no failed/ambiguous create was retried.

| Order | Brief / issue | Read-back state | Dependencies / handoff |
|---|---|---|---|
| 1 | [D — specification #1462](https://github.com/taskqorg/taskq/issues/1462) | waiting; docs / Claude | #1443 closes first; edits owning contract/design before any UI implementation. |
| 2 | [N — navigation #1463](https://github.com/taskqorg/taskq/issues/1463) | held; code / Codex | #1462, #1440, #1455; hold remains until the real M3/M4 foundation handoffs are verified. |
| 3 | [W — conversations #1464](https://github.com/taskqorg/taskq/issues/1464) | waiting; code / Codex | #1463; uses #1440 M3 workspace handoff and supplies O's workspace context. |
| 4 | [O — reviews #1465](https://github.com/taskqorg/taskq/issues/1465) | waiting; code / Codex | #1464; uses the same workspace context, preserving review/execution authority. |
| 5 | [P — panels #1466](https://github.com/taskqorg/taskq/issues/1466) | waiting; code / Codex | #1465; needs working W/O destinations and #1440 M4 tray handoff. |
| 6 | [M — messages/archive #1467](https://github.com/taskqorg/taskq/issues/1467) | waiting; code / Codex | #1466; carries archive-any, permanent one/all deletion and final combined usability acceptance. |

The dependency chain serializes changes to the shared shells. The held foundation
prerequisite cannot become ready merely because the architecture decision closed.
Filing is not implementation, native qualification or runtime acceptance. No
worker was spawned by this session and no capacity/permissions policy changed.
