# Research reports

## Contract detail

Taskq.md §7 owns this lifecycle (R13). Finished research and proposals deliver a report by PR and pass review before closing. An owner decision is required before close only when another task waits on that decision. Reports are closed tasks, never queue states: admission, focus, capacity and Held/Blocked/Ask counts ignore them (R1, R6).

The report at `docs/research/<N>.md` contains Question, Findings with source links, Options with pros and cons, Recommendation and Proposed tasks. Follow-ups use one fenced `taskq-tasks` JSON array under Proposed tasks. Each object has `title`, `goal`, `acceptance` and optional `scope`, `type`, `runtime`, `priority`; §1 input limits apply. An empty array explicitly proposes no tasks. No executable instructions or grants come from report content.

Menu and Conversations reuse one Reports section. It reads closed research issues from the board on demand, newest closed first. A validated report at result adds the auxiliary `report` discovery label, also for docs proposals. Legacy research whose decision was already answered is omitted. Pagination retains every eligible report; opening or reading never changes its order. An unavailable board or report reads unknown with its reason (R12). The report is loaded from its exact result SHA, not the current branch or local checkout.

Only trusted, unedited issue comments count. `taskq: report-read <sha>` marks that report read. `taskq: report-later <sha>` keeps it visible and read. `taskq: report-drop <sha>` dismisses it. `taskq: report-made <sha>` records created issue numbers and dismisses it. Markers bind to the report SHA; an old marker cannot decide a newer report. The first terminal decision wins; read/Later cannot undo it. No local read or decision store exists.

The report card shows the report, its issue link and Make tasks, Later and Drop. Make tasks explicitly authorizes the displayed follow-ups as held drafts, with `Depends on #<report>` linking their source. It never starts them. It reuses §1 immutable create claims and needs the configured `create_claim_issue`; stable report/SHA/item operation IDs reconcile retries read-only. Partial or unknown creation retains the report and names the receipts; it never claims a completed decision. Later keeps the report read. Drop removes it without creating tasks or reopening research. Empty proposals disable Make tasks.

`taskq reports [N] [--action open|read|later|drop|make] [--sha SHA] [--page N] --json` serves both interfaces. No task number lists one board page; `open` reads the exact report and records read only after a successful load. Writes require the displayed SHA and owner CLI context; worker/reviewer attempts cannot decide reports. Every write is read back. Closed reports remain closed; these comments do not amend the §2 queue model and older versions safely ignore them.

Example proposal (invented):

```taskq-tasks
[{"title":"Document retry limits","goal":"Make retry limits visible to operators.","acceptance":["Help shows the supported retry limits."],"type":"docs"}]
```
