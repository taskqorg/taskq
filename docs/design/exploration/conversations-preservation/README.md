# Current-component review · v6

Open [preview/index.html](preview/index.html) through a local HTTP server. It shows the existing menu and Conversations components, with an explicit source-layout comparison and separate View/Settings windows. All fixtures are invented; IPC is intercepted by `fixture.js`. No board, service, provider or PM receives an action.

The earlier handwritten interaction experiment is archival, not the current UI reference: the retained screens use real component controls/validators; the old experiment exercises proposed 10,000-record history and adverse delivery behavior. Neither proves installed/native feature parity. [The functional audit](../conversations-preservation-audit.md) is authoritative for the planning scope; 72 groups are Retain, zero removals approved.

## Build and inspect

`node docs/design/exploration/conversations-preservation/build.cjs` uses this checkout's exact current JS/CSS/JSON and existing pinned npm dependencies installed in the isolated directory named by `TASKQ_REVIEW_LAB` (default points to this design session's scratch directory). It copies sources, changes only review copies and emits complete static pages. `source-manifest.json` pins every reused source and the deliberate changes. The checked-in preview runs without npm or network dependencies.

Serve `preview/` with `python3 -m http.server 57324 --bind 127.0.0.1 --directory docs/design/exploration/conversations-preservation/preview`, then open `http://127.0.0.1:57324/`. The build uses the existing local visualize renderer only to wrap the companion interaction fragment; the shipped static preview needs no renderer.

`node docs/design/exploration/conversations-preservation/check.cjs` checks the source hashes, ledger and selected retained component journeys in Chromium with local fixture receipts. It starts and closes its own local static server. Current-result JSON and eight review screens live in `docs/design/review-evidence/conversations-preservation-2026-10-28/`. Production source is never modified by build/check.

## Deliberate review changes

- Keep existing task rows, global pins, filters, saved views, Completed, task actions and details. Replace only the full question/options reader with a short inline cue and direct exact-thread entry. Details expand in the shared menu content rather than an independently scrolling popup. Other popup uses remain intact.
- Keep all current conversation tools, file/history/watch/archive/delete/detach/Task activity paths and shared answer-card semantics. Give naturally growing input and history one reading owner, with Send tools outside it. Move capture/restore and paging hooks to that owner.
- Scope the selected task's historical receipt feed by board and task; show its result inline rather than adding a second workspace toast. Retain the global menu Answered feed, toast and dismiss/acknowledgement behavior.
- Source comparison loads unmodified current components. The fixture bridge is the same on both sides. It demonstrates native control intent, not OS windows, board transitions, durable save, authority or actual provider execution.

## Known limits to qualify

The source-backed transcript still uses current source pagination and unbounded record accumulation. Do not mistake source reuse for a solved history engine. The separate interaction experiment demonstrates bounded rendering over a known-count fixture; production must implement and qualify byte-cursor page windows, generation fences, anchors and bounded IPC/cache under the plan.

Synthetic save/attach/watch/reply receipts are intentionally local and ephemeral. Frontend validators run; Rust byte/path/signature/storage/authority checks do not. Detach/Return illustrates the source ownership state in the same pane; it creates no native window. Unsupported service effects receive a visible refusal, not an optimistic success. Native Mac/Windows input physics, restart persistence, screen readers and actual two-turn Codex/Claude dialogue remain Not tested.

## Verified review result

The final current-component check passed 18 behavior groups, including 144 geometry cells across question/PM, 350/620/940 px widths, 430/560/740 px heights, 14/16/18/20 px text and both themes. Send was checked by hit testing as well as bounds; document-width clipping alone was not accepted. All 72 source-linked ledger groups are retained in scope; this is not a statement that all 72 passed native or complete UI journeys. No browser page errors occurred.

The independent interaction experiment passed its original 19 behavior groups and 40 geometry cells. Eight current-component screens were visually inspected: compact question with retained task controls, saved views/Undo, complete Settings navigation, and Watch/native session tools. These are distinct review evidence sets; no screenshot proves gesture continuity or native execution. Production-source diff is empty for `src`, `tests`, Cargo and root npm pins; the frozen reference hash is unchanged. Full application `check.py`, real providers, installed/native platforms and user research were not run for this review artifact.

## Earlier presentation revision (v3)

The raw-source appearance in v2 is superseded. Source reuse preserves behavior while the review presentation applies the approved icon system, exact state palette and compact geometry. The native palette is parsed from `src/icon.rs` and projected into both source comparisons; its absence caused the missing colors in the earlier fixture. View-condition `good`/`danger` classes also need an actual scoped color binding, not just palette variables.

New/List belong to the workspace header; command history and Work sessions remain reachable from its one More menu. The empty full-width toolbar is gone. Recognizable tools use bundled Lucide static 1.17.0 SVG with exact accessible names and one unclipped hover/focus hint; answer choice text/IDs, destinations, counts and consequential confirmations retain text. New icon bytes and license are included; production dependency pins are unchanged.

The reader and sidebar own scrolling. Composer/status have no independent scroll. The v6 correction also removes textarea scrolling; an explicitly opened options menu or file preview can need deliberate content handling. One 8 px outer inset and 4–6 px related spacing replace stacked gutters. View/Settings review windows fit their actual content, capped to the available area. Status is fixed; Watch settings disclose inside the reader, with its save row remaining available. File preview focuses its visible area and Escape returns to the prior reading position/control. Per-thread feedback includes owner conversations, so a delayed reply receipt cannot describe the selected different conversation.

The final check additionally verifies rendered palette RGB values, icon accessible names/hints, content-fit gap, preview focus/Escape, a delayed receipt across a thread switch, fixed-control clipping ancestors and a help menu in a 350×430 px / 20 px case. The 144 geometry cells are actual iframe viewports, not CSS sizes overridden by flex growth. Eight current screens were inspected; the earlier eight-screen interaction set is archival, separate evidence.

The same three gates apply to each future slice: functional preservation, interaction behavior, and visual fidelity. Passing one cannot waive another. This remains review-copy code and synthetic native IPC, not installed-product acceptance.

## New chat and Task activity revision (v4)

[Flow and preservation notes](new-chat-and-activity.md) replace the ambiguous plus/Work sessions path. New chat is labelled beside the chat list and saves a draft; its project and first message are grouped before Start chat. The first send has a visible start/recovery state and transitions into that same chat. Existing conversations are named by topic and show Draft/Ready/Working. Task activity is inspection of task executors/reviewers, with verified native destinations or an unavailable reason; it sends no input. Its question action opens the exact task thread. Back preserves the PM draft. Raw session identity remains the runtime fallback when source runtime is missing.

The added check covers that complete path and Start chat in the short narrow large-text case. Screens 07/08 pin the two decisions. The implementation and native/provider boundaries remain unchanged; #1738 is still held.

## Connected PM-to-result revision (v5)

[Complete journey and recovery](pm-to-result.md) connects a PM message, held brief, one assignment, worker progress, exact-revision clarification, automatic return, reviewer and result in the originating chat. Use **PM → result** in the review toolbar, then write the request in the normal PM composer. The Normal / Slow work / Worker failure control affects only this local walkthrough. Task activity remains optional inspection. Existing question components validate and submit both numbered and text answers; every real native effect stays intercepted.

Run `node docs/design/exploration/conversations-preservation/journey.check.cjs` for the focused end-to-end local proof. The check also covers 48 question and 56 task-card geometry cells. Six journey screens form a separate review set. `review_journey` is an invented fixture projection; native PM/task correlation is an explicit implementation gap, not a current source capability. The same 72 retained groups and original native qualification limits apply.

## Input, stable history and click-path revision (v6)

[Current review](input-scroll-review.md) supersedes the earlier fixed-height textarea and nested detail arrangement. One native reading viewport contains history and auto-growing input; Send tools stay reachable. No field grip or inner textarea scroll. A stable record/offset survives append, prepend, resize and thread return; explicit Latest resumes following. Question context is visible; the menu question glyph opens it directly. Nested information becomes labelled inline content, history moves to its own composer icon, and board-close navigation uses the recorded forge mark. The 72-group ledger keeps every capability.

Run `node docs/design/exploration/conversations-preservation/usability.check.cjs` for the added gesture/input/click/depth proof. The review toolbar's Long history and sample-reply controls exercise this same current UI without discarding another draft. The older interaction HTML remains archival and is not the current layout reference. Native/provider/production-scale limits remain unchanged.


The same v6 correction applies economical-space checks D-A12–D-A14. Empty menu/chat search becomes an icon in the existing context row; any active query stays visible. The existing Held action shares the counter flow with an exact current-view scope and remains distinct from its filter. `density.check.cjs` verifies five behavior groups and 32 narrow/large-text geometry cells. [The review](input-scroll-review.md#economical-space-existing-rule-concrete-checks) records the rule, exceptions and cross-section audit.

## Project shortcuts revision (v7)

One left-aligned row contains the recorded board-provider mark, project name and folder mark. Board opens that project’s registered board URL; name retains the existing project overview; folder uses the existing native `folder` intent with the exact registered local root. Back restores the previous task view, query and scroll. No new menu or extra row is added. Missing root disables the folder target with a reason.

`projects.check.cjs` passed four behavior groups and 16 geometry cells: GitHub, self-hosted GitLab, separate roots, active query/scroll return, long names, both themes and large text. Screens 09/10 show the shortcuts and existing overview; the current correction set has ten distinct screens. The local bridge intercepts board/folder intents: no OS folder was opened and native failure handling remains unqualified. Production source is unchanged; #1738 stays held.
