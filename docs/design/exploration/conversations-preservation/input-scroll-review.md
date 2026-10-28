# Input, scrolling and click-path correction (v6)

Owner correction: textarea scrolling and a manual resize grip are unacceptable; automatic growth, fewer clicks, consistent information controls and stable reading matter across the whole prototype. Target requirements are recorded in `taskq.md` Product and `docs/design/ai-workspace.md`. This is review-copy work; no production JavaScript, native task, provider or service was changed. The frozen reference is unchanged outside these explicit owner corrections.

## Findings and resulting behavior

| Finding | Correction | Preserved capability |
|---|---|---|
| Textarea used `resize:vertical`, capped height and `overflow:auto` | Natural height measured in an invisible measuring field; no live-field collapse, grip or inner scrolling | Text, caret, validation, files and source dispatch handlers |
| A very long draft could consume the pane or require another scrollbar | Short input stays at the bottom; long input joins the same reading viewport; Send tools stay outside it | Reading and editing remain possible without another mode/button |
| Source scroll listener still targeted the old detail container | Connect it to the actual reading owner; wheel-up really loads older history | Existing before/after cursors and source receipt validation |
| Raw scroll pixels and an empty loading frame lost the old position | Retain the thread view and visible record/offset; wait for its records before restoring | Independent draft, caret, list selection and reading state |
| Background append or reflow could move the reader | One scroll controller; wheel/key/touch-up pauses follow; resize/prepend/append restore the anchor | User can keep reading and composing |
| New output had no direct return/follow control | One compact latest-message action, above the editor and clear of Send | Only this explicit action resumes following; loading older history is not new output |
| Three nested host disclosures and nested history-source facts | One disclosure per contextual group; nested information becomes labelled inline content | Host/resource/freshness/definitions and exact history interval/source stay available |
| Question context required another Details click | Reason, choices, checked work and evidence are visible with the question | Source choice IDs/order/recommendation, limits and exact revision |
| Command history and Watch required nested menus | History is a direct composer icon; one Conversation info block contains Watch | Fill-only history/Clear, Watch create/edit/remove and native diagnostics |
| Board-close links were generic text | Recorded GitHub/GitLab mark with exact tooltip/accessibility label and URL | This opens the named board to close; it does not directly close a task |

The measuring field is disabled, invisible and excluded from accessibility. It never changes the live textarea value, focus or selection. Measuring by shrinking the live textarea was rejected: even within one animation frame it can clamp the ancestor's scroll position. There is no platform-specific wheel forwarding or global prevention. The reading owner uses native scrolling; sidebar and standalone review menus are independent surfaces rather than nested readers.

The latest action is placed from the actual editor bounds, so an error notice or larger font cannot make it cover Send. Information expansion retains its visible heading. A single-line field does not grow the entire window, and the native outer-window resize grip remains; only the text-field grip is removed.

## Click audit

Counts name clicks on the path, excluding typing and the OS file chooser. Optional technical inspection is never required to answer.

| Intent | Previous review | Current review |
|---|---|---|
| Open a menu question | Task identity → Open question (2) | Existing yellow question glyph (1); identity still opens task controls |
| Read question reason/evidence | Open question → context disclosure | Already visible with the question (0 extra) |
| Answer a numbered option | Choice (1) | Choice (1); same source answer token/revision |
| Answer in free text | Type → Send (1) | Type → Send (1); field grows naturally |
| PM task needs a decision | Open question → choice | Same two actions, then automatic accepted return; Task activity is optional |
| Fill from command history | More → History → entry (3) | History icon → entry (2); never sends |
| Edit Watch | Status → Watch settings → Save (3) | Conversation info → Save (2) |
| Inspect host resources in the menu | Host strip → host → resource facts (3) | Host strip (1); labelled facts are visible together |
| Inspect a task/close on board | Identity → board control | Same path; provider is recognizable without reading a generic label |
| View conditions | View icon → visible fields | Same direct owned View window; no added nesting |
| Settings | Settings → named section | Same two-level navigation; no capability removed |

Primary task controls, consequential confirmation, unknown-delivery checks and board ownership retain their validation. Reducing clicks is not permission to guess acceptance, resend an uncertain operation or close a task from a navigation icon.

## Review coverage

`usability.check.cjs` tests automatic grow/shrink, long paste, real wheel over the shared area, exact record/offset preservation on append/prepend/thread return, explicit follow, visible question context, one-click question entry, recorded GitHub and self-hosted GitLab destinations, direct history, narrow large text and information expansion. It audits Menu, PM, task question, Task activity, Views and all 11 Settings routes. The audited surfaces have no nested disclosures or manual textarea grips. Six screenshots and `usability-checks.json` are in `docs/design/review-evidence/conversations-input-scroll-2026-10-28/`.

Existing `check.cjs` retains its 72-group ledger, 18 behavior groups and 144 geometry cells. `journey.check.cjs` retains the PM → held brief → assignment → worker → question → answer → reviewer → result journey for both recorded runtimes and 104 geometry cells. These are synthetic, local results; actual provider delivery is not claimed.

The Long history review control now opens the same current Conversations UI with a separate invented history thread, preserving the PM draft. Its adjacent new-reply control appends a local sample for trying paused follow. The earlier 10,000-record/adverse-case HTML is kept as an archival engineering experiment in the package, not launched as the current product UI. Its old composer/layout and prior 19/40 result are not a v6 acceptance reference.

## Limits and implementation gate

Stable prototype record keys are scoped to a retained view. Native durable message identities, containing-range reload, byte-cursor progress, bounded DOM/cache/IPC and restart restoration still require the production implementation in UX04–UX06 / Q-A3–Q-A6. The source-backed renderer still accumulates loaded pages; retained per-thread views are not a production cache design. No native smoothness, screen-reader, IME, macOS/Windows physics, real provider or production performance pass is claimed.

UX02/Q-A2 are refined: keep Send tools reachable; a naturally growing long input uses the same reading viewport, rather than gaining its own scroll or manual grip. Native service/readback, R1/R3/R9/R12 and provider adapters remain authoritative. All 72 functional groups remain Retain; no capability removal is approved. The manual field grip and redundant disclosure steps are explicitly replaced by the owner's requested behavior. Task #1738 remains held; no PM message, work dispatch or operational change is authorized.


## Economical space: existing rule, concrete checks

Shared D4 already requires useful whitespace; D5 requires direct primary actions; D11 keeps small actions beside their subjects; D13 protects active work. Stable adaptation forbids opaque frequency-based rearrangement. The owner correction applies those rules rather than adding usage tracking or hiding capabilities. TaskQ-specific D-A12–D-A14 now make them testable across the interface.

- Permanent space is for current information and primary work. Empty secondary search costs an icon, not a row. It expands on explicit intent; focus and any nonempty query remain visible, even with no matches. Clear/empty blur/Escape restore the compact form.
- An action belongs to its subject's existing row when it fits without reducing type or targets. Held admission shares the counter flow as a distinct solid Play action; the Held chip still filters. Its exact count and current-view scope are in its accessible name and tooltip; source task keys/revisions are unchanged.
- An opened settings/configuration page may keep empty fields: they are the current task there. The empty main message composer is also primary. Errors, questions, unknown state, active filters, fixed state choices and required confirmations cannot disappear to save space.
- UI adaptation follows explicit state and intent, not inferred popularity. No automatic control reordering or data removal. Review rows/clicks, keyboard/focus, reflow, applied state and preserved capability together.

| Surface | Review result |
|---|---|
| Menu | Remove the permanent empty Search row and separate Start held row; retain search, all presets/filters/counts, exact admission scope and task controls |
| Conversations list | Search icon shares the New chat row; an active query and its Clear action occupy one contextual row; drafts/selection remain independent |
| Conversation input/history | Primary empty input stays available; height is natural; delivery, active work and latest-output action remain visible without another reader |
| Question and task context | Primary question/context visible; technical facts take one information control; no nested Details |
| Task activity | Task, role, state and actual destination retained; it remains optional inspection, not a required PM detour |
| Views | Purposeful editing surface; source fields stay visible when opened, no new menu row or nested editor |
| All 11 Settings routes | Existing widgets/operational controls retained; nested facts become labelled inline content; empty primary configuration fields remain usable |

`density.check.cjs` adds five behavior groups and 32 menu geometry cells (320/360 px, 430/740 px, 14/16/18/20 px, both themes). It checks focused open/empty Escape, visible no-match query across blur/Settings, Clear, no dedicated Held row, scoped local batch admission, shared chat-search behavior and non-clipped filters/view controls. Screens 07/08 show compact and applied-search states; the full v6 owner review set has eight distinct screens. It changes no real task.
