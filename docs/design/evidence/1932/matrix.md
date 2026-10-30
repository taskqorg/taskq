# macOS requirement-to-evidence matrix

Candidate: `554552daf48d5b413918d54a8a6d0f88efba4020`. R12 applies: a source integration receipt is not a native result. Each row below requires its own observable execution. Native fixture failures F1/F2 are in the report; a capture is partial rendering evidence, never a real-gesture or provider-flow pass. `Not tested` records no native verdict; it is neither a product failure nor a pass. See [report](../../../research/1932.md) for execution limits.

## Integrated scenarios

| Requirement | Native macOS | Required exercise / remaining evidence |
|---|---|---|
| S01 | Not tested | Open normal tasks in all eight states, multi-digit IDs/counts, long titles and multiple project groups |
| S02 | Not tested | Inspect first/subsequent and collapsed/expanded headings in menu, sidebar, Settings home and each inner route |
| S03 | Not tested | Open GitHub and self-hosted GitLab boards; project name; registered folder; missing/denied root; Back |
| S04 | Not tested | Empty/open/typed/no-match/cleared search; Escape/blur; Held batch action under changed filters |
| S05 | Fail: F2 native guard; remaining routes/interactions Not tested | Settings, Appearance, Notifications, Activity history, Hosts/resources, Work mode, Agents/access, Projects, Delivery, Services/support, Recovery, About |
| S06 | Not tested | Fresh idle/running hosts, duplicate/conflicting executions, stale/missing/partial host, codex-app, other runtime, execution-only host |
| S07 | Not tested | Toggle ordinary/Ask/failed/held/blocked/review details by click/Enter/Space; hover, refresh, outside click, Escape/Close |
| S08 | Not tested | Wheel/trackpad starts over panel title, summary, controls and boundaries in both directions; switch open task |
| S09 | Not tested | Read a current question, an answered older question and a standalone/detached task thread |
| S10 | Not tested | Activate forge, number, runtime and explicit native/progress targets for local Claude/Codex/codex-app, remote Claude/Codex and no-link cases |
| S11 | Not tested | Inspect current worker, reviewer, unknown/stale task and ended/replaced attempt; open closed pinned transcript and archived task |
| S12 | Not tested | Write PM -> inspect brief -> assign once -> see worker -> answer clarification -> reviewer -> result |
| S13 | Not tested | New chat vs continue PM; Task activity; New from draft; rename/history/Watch; switch owner/task/event threads |
| S14 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | Type/paste/restore one line and very long multiline/Unicode/IME text with files; resize and change font |
| S15 | Not tested | Read 10 MiB, 100 MiB and 10,000-variable-height-record histories; short/hidden-only pages; long single/oversized records; UTF-8/group seams |
| S16 | Not tested | Prepend/append, delayed image/layout changes, tool expansion, font/resize, switch/reopen while reading older text |
| S17 | Not tested | Send choice/text/worker message; question changed/already answered; long reply; native writer busy; delayed/failed/unknown receipt |
| S18 | Not tested | Timeout/restart mid-send, out-of-order foreign response, rejected save, board/remote outage and worker replacement |
| S19 | Not tested | Owner/worker messages, grouped and expanded tools, question/event record, list activity, sent request and watch/turn facts |
| S20 | Not tested | Missing/invalid/zone-less time; seconds vs milliseconds vs ISO; future schedule; minute/day boundary and remote clock case |
| S21 | Not tested | Summary A with incoming/closed/Ask/Failed mixtures, Blocked detail, first Held start, retry/requeue/resume, restart, legacy gap, zero/unknown/partial, rare event, overflow and pause |
| S22 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | 320/360 menu and 350/620/940 workspace widths; 430/560/740 heights; 14/16/18/20 type; themes; real native scales; long IDs/files/code |
| S23 | Not tested | Keyboard-only reading/answering, screen reader, copy/selection, scrollbar drag, attachments/refusal, archive/Undo/restore/delete, detach/return and owner change |
| S24 | Not tested | Restart the isolated client with drafts, archived threads, pending operation and saved reading anchor; replay older/newer snapshot after answer |

## Complementary native criteria

| Requirement | Native macOS | Required exercise / remaining evidence |
|---|---|---|
| Q-A1 | Not tested | With an open question preview, wheel/trackpad over title, summary, actions and preview boundaries moves the intended menu content in both directions; rows stay reachable. Test the actual gesture, not `scrollTop` assignment. No inner question scrollbar. |
| Q-A2 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | At the start, middle and end of long content, Send tools remain inside the selected pane without `scrollIntoView()`. Short input stays at the bottom; a naturally growing long input uses the sole reading viewport without an inner scroll or manual grip. Sidebar and reading viewport scroll independently; header remains stable. |
| Q-A3 | Not tested | Prepending, appending, expanding content, font change and resize preserve the same visible record/offset within 2 CSS px after layout settles; no delayed second jump. Returning to a previously read thread reloads the containing range. |
| Q-A4 | Not tested | While reading older text, new output does not move the reader. A visible action reaches latest; only an explicit acknowledgement changes unread-event state. Short pages and pages containing only ignored records still reach earlier history. |
| Q-A5 | Not tested | A 10 MiB source (existing D-A3), a 100 MiB stress source and 10,000 variable-height visible records remain traversable to the beginning without omissions/duplicates. Mounted nodes/cache plateau under the configured window; report the bound. Include long single messages and the documented oversized-record refusal. |
| Q-A6 | Not tested | Proposed reference-host target: visible input/selection feedback p95 ≤100 ms; no UI main-thread task >50 ms caused by a normal page append/prepend after warm-up. Capture frame times during continuous wheel scrolling; target display refresh budget, report misses. Report exact host, renderer, scale, fixture, cold/warm state and I/O separately; no hard-coded sleep or “16.7 ms” label counts as evidence. |
| Q-A7 | Not tested | Question #A cannot display answer #B's receipt, including two questions on the same board, late replies and a thread switch while pending. One submission has one visible status and never dispatches twice on timeout/reopen. Accepted transport is not displayed as agent completion. |
| Q-A8 | Not tested | Free text, numbered choice, changed question, native writer busy, definite refusal and unknown result preserve identity, revision and independent next draft/files. Navigation/New/Archive/Mark read/Close do not dispatch a turn. IME, Enter/Shift+Enter and focus survive updates. |
| Q-A9 | Not tested | Keyboard-only reading/answering, screen-reader attribution, text selection/copy and scrollbar dragging work with virtualized history. Do not announce the entire history on each prepend/poll. Secondary controls, diagnostics/watch, files, archive/Undo/delete and detach/return remain reachable. |
| Q-A10 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | No horizontal overflow or clipped actionable content at menu widths 320/360 and workspace widths 350/620/940 CSS px, 430/560/740 px heights, fonts 14/16/18/20, both themes and native 1×/2× scales. Include unbroken identifiers, long choices, files, code, unknown and empty states. Check scrollWidth on relevant containers, control rectangles and visual output; root clipping is not success. |
| Q-A11 | Not tested | Inject delayed/out-of-order/failed reads, partial transcript writes, source shrink/replacement, save failure and missing receipt. The selected thread and draft remain coherent, navigation/Close keep working and recovery cannot replay an uncertain send. |
| Q-A12 | Not tested | On an explicitly disposable local owner session for each supported runtime, send two PM messages, see the two corresponding native replies, revisit older history, switch threads and reopen. Same native session; no unintended worker/board effect. Qualify restart and one-writer refusal independently. |

## UX30 implementation and native proof

Source destinations and implementation owners remain in [UI30](../../ui-polish.md); source integration is in [integration.md](integration.md).

| Requirement | Native macOS | Required outcome |
|---|---|---|
| UX30-01 | Not tested | Eight status marks have one centered slot and common text edge; approved icons/colors remain |
| UX30-02 | Not tested | Project/section headings are distinct, with more space above than below; rows remain compact |
| UX30-03 | Not tested | Project board, overview and registered folder are separate direct targets with Back continuity |
| UX30-04 | Not tested | Empty search and scoped Held admission use existing rows, preserving active scope/query |
| UX30-05 | Not tested | All Settings are coherent and retain controls, factual distinctions and review boundaries |
| UX30-06 | Not tested | Host totals first; overall Codex/Claude totals second; shared expanded columns and truthful uncertainty |
| UX30-07 | Not tested | Ordinary/question details share one inline raised/state-accented panel; no task popover or nested scroll |
| UX30-08 | Not tested | Task names/state/age are not repeated inside child details; real historical content remains |
| UX30-09 | Not tested | Forge/title -> exact issue; number -> inline details; runtime -> exact task-agent view |
| UX30-10 | Not tested | Native session, remote read-only progress and unavailable destinations are explicit and consistent |
| UX30-11 | Not tested | PM message -> brief -> assignment -> worker -> clarification -> reviewer -> result is one intelligible flow |
| UX30-12 | Not tested | New chat and Task activity have clear purposes; browsing never creates or resumes a session |
| UX30-13 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | Composer grows automatically, no grip/inner scroll; Send and attachments remain reachable |
| UX30-14 | Not tested | Long history remains bounded/traversable; append/prepend/reflow retain reading anchor and explicit follow |
| UX30-15 | Not tested | Delivery, agent activity and result are distinct; slow/failure/unknown recovery never replays uncertain input |
| UX30-16 | Not tested | Running-worker message is available before a question via existing board steer; ended/reviewer limits are explicit |
| UX30-17 | Not tested | Relative times appear on messages, tools, questions/events, activity rows and status/watch facts |
| UX30-18 | Not tested | Exact local date/seconds/time zone on hover/focus; source units and missing provenance are truthful |
| UX30-19 | Not tested | Relative labels update without redraw/reordering, focus loss or scroll movement |
| UX30-20 | Not tested | Selected A: six event stacks for incoming/completed/questions/failures with preserved counter meanings |
| UX30-21 | Not tested | New = first queue admission; legacy missing data stays unknown; known old history remains visible |
| UX30-22 | Not tested | One Rust icon renderer; no extra normal ticks/caps/underline; native counts/overflow/platform behavior retained |
| UX30-23 | Not tested | Every existing capability and newer feature is mapped, retained and regression-checked |
| UX30-24 | Not tested | Integrated native macOS/Windows proof packet, with exact provenance and unresolved gaps, for independent owner review |

## Extended preservation ledger

Every row remains Retain. Original destination, owner and risk check: [84-group canonical ledger](../../ui-polish-capabilities.md). These are verdicts for this candidate, not a copy of historic passing checks. No source presence, closed task or browser result fills a native cell.

| Capability | Native macOS | Current destination |
|---|---|---|
| M01 | Not tested | Same counter strip and separate View conditions |
| M02 | Not tested | View window → Projects |
| M03 | Not tested | Separate View window; same menu shortcuts |
| M04 | Not tested | View window → Saved views |
| M05 | Not tested | View → Display |
| M06 | Not tested | Menu above ordinary task results; shared scroll |
| M07 | Not tested | Menu search; Clear/Reset |
| M08 | Not tested | Completed view |
| M09 | Not tested | M3.9/M3.10: forge/title → issue; number → inline details; runtime → exact task-agent view; explicit native/progress links retained |
| M10 | Not tested | M3.9/M3.10: forge/title → issue; number → inline details; runtime → exact task-agent view; explicit native/progress links retained |
| M11 | Not tested | Existing row age; expanded task Details |
| M12 | Not tested | Inline task detail; full context in Conversations/Board |
| M13 | Not tested | Existing task actions and scoped Start held |
| M14 | Not tested | Task actions; secondary actions stay available |
| M15 | Not tested | M3.9/M3.10: forge/title → issue; number → inline details; runtime → exact task-agent view; explicit native/progress links retained |
| M16 | Not tested | M3.9/M3.10: forge/title → issue; number → inline details; runtime → exact task-agent view; explicit native/progress links retained |
| M17 | Not tested | Existing header locations and parent-only breadcrumb |
| M18 | Not tested | Single Conversations entry; unread badge stays source-driven |
| M19 | Not tested | Separate existing controls in menu and workspace |
| M20 | Not tested | Existing native shell/corner grip and Appearance |
| M21 | Not tested | Projects page and existing setup review |
| M22 | Not tested | Existing compact host line and expansion |
| M23 | Not tested | Settings → Work mode; existing operation workspace |
| M24 | Not tested | Existing emergency/recovery review |
| M25 | Not tested | Settings → Appearance in its own window |
| M26 | Not tested | Settings → Notifications |
| M27 | Not tested | Activity history; Summary A event stacks via icon.rs; exact Blocked detail and unchanged current counter order |
| M28 | Not tested | Settings → Agents/access |
| M29 | Not tested | Settings → Delivery |
| M30 | Not tested | Settings → Services/support; existing reviewed operations |
| M31 | Not tested | Settings → About/updates |
| M32 | Not tested | One menu reading scroll; separate window routes |
| C01 | Not tested | Existing unified Conversations list |
| C02 | Not tested | Chat list: New chat saves a draft; existing topics continue directly; narrow Chats/Back |
| C03 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | Existing durable draft/files/caret; automatically growing composer in the one thread reader, reachable Send |
| C04 | Not tested | New chat shows its registered project before Start chat; recorded runtime stays fixed after launch |
| C05 | Not tested | Same selected thread/composer |
| C06 | Not tested | Composer secondary action; explicit new destination |
| C07 | Not tested | Composer history icon; one direct menu; fills only |
| C08 | Not tested | Existing editable conversation name |
| C09 | Not tested | Existing composer file picker and drag/drop |
| C10 | Not tested | File chips → preview; Remove local reference |
| C11 | Not tested | Task activity at existing Work sessions route; reading creates/resumes nothing; exact agent view plus separately named native/progress links |
| C12 | Not tested | Conversation tools and separate window |
| C13 | Not tested | Launch status → Cancel queued launch |
| C14 | Not tested | One thread-local status; Check delivery |
| C15 | Not tested | Inline actionable failure with retained draft/files |
| C16 | Not tested | Conversation tools/Archive list; clear local scope |
| C17 | Not tested | Archive → Delete permanently → scoped confirmation |
| C18 | Not tested | Existing selected event/thread controls |
| C19 | Not tested | Conversation info → inline Watch controls; one disclosure |
| C20 | Not tested | Single reading viewport; Load earlier recovery retained |
| C21 | Not tested | Measured contiguous page window first; optional maintained virtualizer after spike |
| C22 | Not tested | One status per operation, distinct activity indicator |
| C23 | Not tested | Existing header and Work sessions/operation review routes |
| C24 | Not tested | Existing shortcuts plus fixed input; source labels retained |
| A01 | Not tested | Existing shared answer-card inside task reading viewport |
| A02 | Not tested | Question card → Checked & context; PM recommendation separate |
| A03 | Not tested | Same question card and shared composer |
| A04 | Not tested | Original source text + explained available actions |
| A05 | Not tested | Explicit effect review; existing two-step close flow |
| A06 | Fail: [F1 / #1991](../../../research/1932.md#reproduced-native-failures); remaining behavior Not tested | Shared validated answer composer |
| A07 | Not tested | Updated question → explicit review before answer |
| A08 | Not tested | One selected-task outcome; global Answered remains available |
| A09 | Not tested | Global menu Answered kept; task workspace filters by task identity |
| A10 | Not tested | Reuse same exported components/native commands |
| X01 | Not tested | Existing Preact/Signals/Wouter/CoreQ and published interfaces |
| X02 | Not tested | No replacement local task database or transcript store |
| X03 | Not tested | One view model over existing native Codex/Claude adapters |
| X04 | Not tested | Retain existing pages; simplify entry/hierarchy, not scope |
| X05 | Not tested | F0 parity gate, then independently usable slices |
| X06 | Not tested | This ledger is a mandatory planning input |
| N01 | Not tested | Reports in menu/Conversations |
| N02 | Not tested | Shared task composer and canonical answer/steer/requeue transport |
| N03 | Not tested | Current question + immutable receipt + prior context |
| N04 | Not tested | Pinned row read-only transcript, Completed, Archive/Restore |
| N05 | Not tested | Shared Summary counters and native slot |
| N06 | Not tested | Task-agent thread with attempt-fenced live tail |
| N07 | Not tested | Same thread via E2E pull-only host link |
| N08 | Not tested | Recent grouped list and Archive |
| N09 | Not tested | One shared native snapshot; sending until original op-id readback |
| N10 | Not tested | Append-only task conversation in log order |
| N11 | Not tested | One collapsed consecutive step line, count + files/commands/tests; expandable records |
| N12 | Not tested | Durable board/task provenance → shared Summary/History |

## Eight daily journeys

The v2 baseline audit defines these journeys. Current fluent-answer/file/closed-pin rules override incidental historical mock behavior.

| Journey | Native macOS | Missing evidence |
|---|---|---|
| 1. A question needs me | Not tested | Integrated native execution and its attributable readback |
| 2. I want to ask the PM | Not tested | Integrated native execution and its attributable readback |
| 3. I need an old decision | Not tested | Integrated native execution and its attributable readback |
| 4. I cannot decide now | Not tested | Integrated native execution and its attributable readback |
| 5. Something takes time or fails | Not tested | Integrated native execution and its attributable readback |
| 6. I need to organize work | Not tested | Integrated native execution and its attributable readback |
| 7. I need more control | Not tested | Integrated native execution and its attributable readback |
| 8. I need to change the service | Not tested | Integrated native execution and its attributable readback |
