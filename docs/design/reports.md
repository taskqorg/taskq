# Research reports

## Contract detail

Taskq.md §7 owns this lifecycle (R13). Finished research and proposals deliver a report by PR and pass review before closing. An owner decision is required before close only when another task waits on that decision. Reports are closed tasks, never queue states: admission, focus, capacity and Held/Blocked/Ask counts ignore them (R1, R6).

The report at `docs/research/<N>.md` contains Question, Findings with source links, Options with pros and cons, Recommendation and Proposed tasks. Follow-ups use one fenced `taskq-tasks` JSON array under Proposed tasks. Each object has `title`, `goal`, `acceptance` and optional `scope`, `type`, `runtime`, `priority`; §1 input limits apply. An empty array explicitly proposes no tasks. No executable instructions or grants come from report content.

Menu and Conversations reuse one Reports section. It reads closed research and docs issues from the board on demand, newest closed first. A validated report adds `Report: docs/research/<N>.md` to the accepted result comment, also for docs proposals; unrelated docs are omitted. Discovery requires no later label write. Legacy research whose decision was already answered is omitted. More uses the last close-time/issue-number cursor, so decisions removing earlier rows cannot skip unseen reports. Opening or reading never changes order. An unavailable board or report reads unknown with its reason (R12). The report is loaded from its exact result SHA, not the current branch or local checkout.

Only trusted, unedited issue comments count. `taskq: report-read <sha>` marks that report read. `taskq: report-later <sha>` keeps it visible and read. `taskq: report-drop <sha>` dismisses it. `taskq: report-making <sha>` reserves Make tasks before any creation; `taskq: report-made <sha>` records created issue numbers and completes it. The first Drop or make-intent wins across hosts: a make-intent keeps the report visible until its receipts reconcile, and a concurrent Drop cannot hide newly created drafts. These are report metadata, not queue transitions. Markers bind to the report SHA; an old marker cannot decide a newer report. Read/Later cannot undo a terminal decision. No local read or decision store exists.

The report card shows the report, its issue link and Make tasks, Later and Drop. Make tasks explicitly authorizes the displayed follow-ups as held drafts, with `Depends on #<report>` linking their source. It never starts them. It reuses §1 immutable create claims and needs the configured `create_claim_issue`; stable report/SHA/item operation IDs reconcile retries read-only. Partial or unknown creation retains the report and names the receipts; it never claims a completed decision. Later keeps the report read. Drop removes it without creating tasks or reopening research. Empty proposals disable Make tasks.

`taskq reports [N] [--action open|read|later|drop|make] [--sha SHA] [--page N|--before TIME:N] --json` serves both interfaces. No task number lists one board page; `open` reads the exact report and records read only after a successful load. Writes require the displayed SHA and owner CLI context; worker/reviewer attempts cannot decide reports. Every write is read back. Closed reports remain closed; these comments do not amend the §2 queue model and older versions safely ignore them.

Example proposal (invented):

```taskq-tasks
[{"title":"Document retry limits","goal":"Make retry limits visible to operators.","acceptance":["Help shows the supported retry limits."],"type":"docs"}]
```
