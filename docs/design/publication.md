# GitHub publication waits

Implemented detail of the GitHub `pr` route (#1820, pilotq #106). The decisions are in [taskq.md § 7](../../taskq.md#7-worker-reviewer-publication): conflict is `main-moved`, unknown is pending, failed CI or conflict requeues, and a pending GitHub publication stays in review. This page records how the service applies them so that no publication waits silently. Chain check: `github_queue_qualifies_the_merge_result_and_preserves_unknowns` in `src/board/mod.rs` and `publication_stall_shows_its_reason_and_age_then_requeues_on_conflict` in `src/service.rs`.

Each service pass either moves a published task (merged, enqueued, requeued, asked) or names why it waits. A task never stays in review past one pass without a merge-queue entry or a stated reason.

- PR head is not the result SHA (for example, a rebase pushed in review): the PR is not at the result SHA and never merges, so `main-moved` names the PR and both short SHAs. The worker reviews the head and delivers it as a new result.
- PR conflicts with `main` (`mergeable` is `CONFLICTING`, before enqueue or after a `merge_conflict` dequeue): `main-moved` names the conflict files. They are the files changed both on the PR since its merge base and on `main` since that base, read from the board's compare API (Git-free; five at most, then `+N more`; unreadable: `conflict files unknown`).
- Dequeued as `merge_conflict` but the PR is now `MERGEABLE` (the conflicting entry ahead of it left): no conflict remains, so the service enqueues the same head again. The merge queue builds on current `main`; no rebase is needed. Mergeability not computed yet: pending with that reason.
- Enqueue refused for another reason (for example, a required check still running): a failed check at the result SHA is failed CI (`publication-failed`, naming it); otherwise pending with the refusal and the check state.
- Each pending pass sets the task's status row `publication` (`reason`; `since`, the `publish` entry time; `age_s`) and its `wait` text `Publication waiting <age> · <reason>`, which the menu and dashboard show. Telemetry records one `checks` stage, outcome `retried`, per distinct reason, never one per pass.
