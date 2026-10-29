# Multi-stage tasks

Accepted under [taskq.md § 2 Stages](../../taskq.md#stages) (#1880). One task carries a whole chain of steps. An attempt finishes one or more stages, parks with `later`, and the next attempt continues at the first open stage. § 2 transitions stay the behavioral authority.

## Contract detail

- Stage: one Acceptance item of the issue body, a line `A<k>. <text>` (optional `- ` or `* ` bullet). The task's stages are its Acceptance items in body order; `n` is their count. A task without Acceptance items has no stages.
- Record: a `later` entry whose text holds a line `Stages: A<k> [A<j>..]` records the finished stages. The line names the whole finished set so far, so each record replaces the one before; `Stages: none` clears it. A malformed line (any token other than `A<k>`) changes nothing. Any accepted `later` form records it: worker park with a blocker (row 23), timed `later until <UTC>` or plain `later` (row 24). A `later` without the line keeps the recorded set.
- CLI: `taskq later N --stage A<k> [--stage A<j>..] [--until UTC|--blocker #M] [--text T]` appends the `Stages:` line to the text after the § 5 `Reason:`/`Next:` checks. A worker stage park may omit `--until` and `--blocker`; then the task waits in `later` for the owner or PM. Each `--stage` must name an Acceptance item of the issue.
- Fold: `Task.stages` holds the recorded set, sorted by number. It survives `ready`, every `requeue`, the next `spawn` and `result`; only a later `Stages:` line changes it. N-1 reads the line as plain `later` text and folds the same state, retries and time.
- Retries: a stage park is a `later` and never counts toward the retry limit. The timed resume is the service's owner-side `requeue` (row 22, `retries = 0`); requeue and `gone` counting stay unchanged.
- Brief (§ 7): the board-backed task context names the finished stages and the first open one. The next worker continues in the same `.worktrees/taskq-<N>` on branch `taskq-<N>` (§ 7 rule 2) and hands in a `result` once every stage is done, or parks again with `--stage`.
- Report (R6): a task with a recorded set shows `stage k/n`, where `k` is the position of its first open stage (all done: `n`). Status text adds it to the State cell and the `Later:` entry; `status --json` rows add `stage` `{"k", "n", "done"}`; the tray shows `Stage k/n` on the task row. Tasks without a recorded set add nothing.
- The issue shows the finished stages through the board's own `later` comment and its `Stages:` line (R1); taskq never edits the issue body.

## Example

```
taskq: later until <UTC> attempt 41
Reason: 168 qualified hours before the A3 soak
Stages: A1 A2
```

The service writes `requeue` at that board time, the next attempt's brief reads `Stages done: A1 A2; continue at A3 (stage 3/4)`, and status shows `stage 3/4`.
