# Research reports

## Results expansion target

Owner steer on #1940 widens this surface to Results: reports are one kind of completed result. This target is not yet implemented. A finished task must appear once, with a SHA-bound seen state, in menu attention and Conversations; already seen results add no new attention. Background-collected summary, checks, open items, PR and evidence must support instant list, open and next/previous navigation. Ordinary task reactions are Accept, Follow-up and Reopen; report reactions retain Make tasks, Later and Drop.

The shared use case is: work finishes; the service collects the result; the operator notices it where they already work, reads the cached decision card and chooses the appropriate reaction. Accept acknowledges a delivered result; Follow-up proposes new work; Reopen requests more work on the original outcome. A read or acknowledgement is not permission to start work. Missing or stale data remains unknown.

Reopen needs an owner decision before lifecycle implementation: §2 currently permits owner `requeue` only from open states. Resume the original closed issue with an explicitly specified transition and compatibility plan, or create a linked held successor while preserving the original closure. The task's ask records the choice; no closed-task transition is changed by this target.

## Contract detail

Taskq.md §7 owns this lifecycle (R13). Finished research and proposals deliver a report by PR and pass review before closing. An owner decision is required before close only when another task waits on that decision. Reports are closed tasks, never queue states: admission, focus, capacity and Held/Blocked/Ask counts ignore them (R1, R6).

The report at `docs/research/<N>.md` contains Question, Findings with source links, Options with pros and cons, Recommendation and Proposed tasks. Follow-ups use one fenced `taskq-tasks` JSON array under Proposed tasks. Each object has `title`, `goal`, `acceptance` and optional `scope`, `type`, `runtime`, `priority`; §1 input limits apply. An empty array explicitly proposes no tasks. No executable instructions or grants come from report content.

The service collects closed research and docs issues in the background, independently of execution supervision. Each ordinary pass schedules a due collection; observed close events invalidate it immediately. The collector reuses unchanged issue projections only after reading beyond their board update second, and immutable content by result SHA. Its rebuildable projection travels in `status.json`; it is never decision truth (R1). Collection errors retain previous rows with an unknown reason. Missing, incomplete or older-than-`ttl` data reads unknown; an unavailable report has its own reason. Collection, list and open timings use built-in telemetry (R12).

Menu, Conversations and CLI read this collected projection without board calls or a UI polling timer. A validated report adds `Report: docs/research/<N>.md` to the accepted result comment, also for docs proposals; unrelated docs are omitted. Discovery requires no later label write. Legacy research whose decision was already answered is omitted. Rows remain newest closed first; duplicate project registrations cannot duplicate reports. More uses the last close-time/issue-number cursor, so decisions removing earlier rows cannot skip unseen reports. Opening or reading never changes order. Content always belongs to its exact result SHA, never a moving branch or local checkout.

The operator flow has four steps, each with one purpose:

1. Notice: unread reports appear in menu Attention and the Conversations thread list, and join its new-item badge like unread questions. This brings the decision to the operator's existing workplace. Queue and admission counts still exclude reports.
2. Understand: one click opens the collected card with Question, Recommendation and Proposed tasks first, as readable text and task titles. This exposes the decision without requiring a long document read. Full report expands in one action.
3. Decide: Make tasks, Later and Drop are direct buttons, without an intermediate confirmation or PM relay. Make tasks creates only the displayed held drafts; Later records read and keeps the report; Drop dismisses it. Read acknowledgement is asynchronous after successful local display, so a board write never delays reading. Its failure remains visible and unread stays unconfirmed.
4. Return: Back restores Conversations and its draft. Successful decisions update the current view from their readback and wake background collection; failures retain the card and receipts for reconciliation.

In-flight receipts overlay only their own SHA's markers, preserving source content and close order. The collected `comments_through` revision retires an overlay once collection reaches its readback; an older acknowledgement cannot replace a newer report or reservation.

Only trusted, unedited issue comments count. `taskq: report-read <sha>` marks that report read. `taskq: report-later <sha>` keeps it visible and read. `taskq: report-drop <sha>` dismisses it. `taskq: report-making <sha>` reserves Make tasks before any creation; `taskq: report-made <sha>` records created issue numbers and completes it. The first Drop or make-intent wins across hosts: a make-intent keeps the report visible until its receipts reconcile, and a concurrent Drop cannot hide newly created drafts. These are report metadata, not queue transitions. Markers bind to the report SHA; an old marker cannot decide a newer report. Read/Later cannot undo a terminal decision. No local read or decision store exists.

The report card shows the report, its issue link and Make tasks, Later and Drop. In Conversations, an open report gets one scrollable reading area; returning to the list or collapsing Reports restores the conversation and preserves its draft. Make tasks explicitly authorizes the displayed follow-ups as held drafts, with `Depends on #<report>` linking their source. It never starts them. It validates proposals and the board's `create_claim_issue` before reserving Make tasks: without the key it first settles the claim issue as `taskq claim-issue` does (reuse, find or create; key written and read back), on GitHub and GitLab alike, and a refused or unknown claim issue reserves nothing. It then reuses §1 immutable create claims; stable report/SHA/item operation IDs reconcile retries read-only. Partial or unknown creation retains the report and names the receipts; it never claims a completed decision. Later keeps the report read. Drop removes it without creating tasks or reopening research. Empty proposals disable Make tasks.

`taskq reports [N] [--action open|read|later|drop|make] [--sha SHA] [--page N|--before TIME:N] --json` serves both interfaces and PilotQ callers. No task number lists one collected page; `open` returns exact cached content without acknowledging read. The UI separately sends `read` after successful display. Writes require the displayed SHA and owner CLI context; worker/reviewer attempts cannot decide reports. Every write is read back and wakes collection; actions validate current board state even when the displayed cache is stale. Closed reports remain closed; these comments do not amend the §2 queue model and older versions safely ignore them.

Example proposal (invented):

```taskq-tasks
[{"title":"Document retry limits","goal":"Make retry limits visible to operators.","acceptance":["Help shows the supported retry limits."],"type":"docs"}]
```
