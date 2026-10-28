# Connected PM-to-result walkthrough (v5)

Owner request: complete the task journey in the prototype and remove unclear detours. Real tasks and agents must remain unstarted. This is a proposed interaction and a local simulation, not a change to R1–R13 or evidence of native execution. The current app has no verified PM-origin/task correlation in this renderer; the review's `review_journey` field is fixture data only.

## One continuous path

| Person's intent | Visible action | What changes | What stays |
|---|---|---|---|
| Tell PM what to do | Type an ordinary request in the existing PM chat and Send | PM prepares a short brief; a local held task #101 appears | Same chat, project, runtime and input controls |
| Delegate the reviewed brief | 1 · Assign task | One simulated worker starts; menu and task activity refer to that task | Brief and original instruction remain readable |
| Continue using the tool | Read history, write a draft or switch chats | Worker progress stays on its originating PM card | No forced Task activity visit, no extra session creation |
| Resolve one decision | Answer question beside the task progress | Exact #101 question opens with source choice IDs and shared answer controls | Back to PM is explicit; both independent drafts survive |
| Submit the answer | Pick a source choice, or write text and Send | Exact-revision receipt resumes that task; accepted answer is named | Automatic return to the same PM after acceptance; no manual refresh |
| Receive the result | Stay in the PM chat | Worker → separate reviewer → result and review summary | One task/card; no fake publication, merge or production qualification |

Two deliberate actions occur before work: send the request and assign its inspectable brief. A normal numbered question choice submits directly. Free text uses the existing Send. Task activity, native agent chats, the board, New chat, Settings and file/Watch/history tools remain optional/reachable; none is required to finish the default path. The compact menu shows the same task/question and retains its original task controls. No second PM intake task was created.

## State and recovery

PM replying/ready describes only the PM turn. Worker progress, waiting for the owner, review, result and failure describe the assigned task in a single inline card. Delivery feedback stays at the source composer and does not become worker progress. The reading pane owns all added content; task actions/history introduce no scroll container or floating panel. Detailed events disclose under Task activity within the card.

The review toolbar's PM → result action starts a fresh invented walkthrough. Normal, Slow work and Worker failure are fixture conditions outside the product UI. They are not settings or permissions. The existing long-history/failure lab remains separate and is labelled as an experiment.

| Case | Expected behavior |
|---|---|
| Keep for later | Task stays held; no worker starts |
| Duplicate assignment | Same generation/task accepts assignment once |
| Slow worker | Input stays usable; elapsed waiting is not a percentage or false result |
| Failure with confirmed worker exit | Clear error; Retry retains task identity and creates another simulated attempt |
| Unknown delivery/execution | Existing adverse-case lab: read back; never blindly resend/retry |
| Switch chat before question/result | No foreign card or forced navigation; return shows the current correlated state |
| Leave the question before answering | Back restores PM draft; reopening restores the answer draft |
| Question revision changes | Source validator/receipt refuses the old answer; worker remains paused |
| Reset during delayed work | Generation fence rejects every old timer; only local fixture state is cleared |
| Short/narrow window or large text | Shared question reader scrolls; composer/Send stay reachable with no horizontal overflow |

The fixture resets at page reload; it cannot prove restart persistence. Source comparison deliberately lacks the new connected card; it is the current source, not a pretend implementation of this proposal. Clicking an intercepted external link shows its destination and performs no real navigation.

## Preservation and implementation boundary

The 72-group audit is unchanged: all Retain, no removal approved. This slice adds a contextual shortcut to existing task question controls and a return destination; it does not replace task lifecycle, session ownership, archive/delete guards, files, saved views, Settings, native storage or provider adapters. Existing menu start/stop/later shortcuts also update the same simulated task.

Before implementation, define and qualify the real association between owner conversation, board/task, attempt, exact question revision and result receipt. PM prose alone is not task truth (R1/R3/R12). An execution service readback, not a front-end timer or inferred text, must drive the native state. Return to PM only after an accepted correlated receipt; preserve context on stale/refused/unknown outcomes. A production result must name its evidence and publication/owner-acceptance state separately. Keep provider-specific native commands behind the existing Codex/Claude adapters; the shared UI must receive equivalent normalized records. This fixture tests identical UI logic with two recorded runtime IDs, not those adapters.

Claude's existing technical dispositions remain applicable: delivery ≠ activity; immutable input/operation identity; generation and dispatch fences; unknown means read back before resend; cursor progress and bounded page windows for history. No fresh Claude consultation or feature-parity approval is claimed for this visual iteration.

## Review evidence

`journey.check.cjs` exercises the complete path for both recorded runtimes (numbered choice and free-text answer), duplicate assignment, held choice, independent drafts, a foreign chat, exact-revision refusal, failure/retry and a delayed event after reset. It also checks 48 question geometry cells and 56 task-card geometry cells across seven phases. `check.cjs` retains the 18 behavior groups/144 geometry cells and the 72-group source audit. Six distinct journey screens and JSON live under `docs/design/review-evidence/pm-journey-2026-10-28/`; the earlier eight-screen preservation set is a separate reference, not an additional variant sheet.

Native macOS/Windows behavior, actual provider execution, durable correlation, production performance/accessibility and nontechnical user testing remain Not tested. The frozen visual reference and production source/dependency pins are unchanged. #1738 remains the single held intake; no PM message or real work was started.
