# Worktree tidy and root freshness

Owner decision 2026-10-23 (W2 + P2): TaskQ owns `<root>/.worktrees` and the Git worktree registry of each root in `<install>/projects`. It tidies them once an hour, fast-forwards the root only on `main`, never resets, and only reports worktrees it does not own. [taskq.md § 4 step 2](../../taskq.md#4-service) links here; where this note differs, taskq.md wins.

## Why

Before this decision the service removed only clean `.worktrees/taskq-<N>` of closed tasks on the attempt host (§ 7 close). A Mac survey on 2026-10-23 found:

- reviewer worktrees `.worktrees/review-<N>` 3 to 8 days old that no code removes;
- worktrees of a root without a service (csgo: 9, two weeks old);
- 16 registry entries whose directory is gone, and 17 `taskq-*` branches without a worktree in one root;
- about 28 GB under `.worktrees` across four roots;
- root checkouts 99 and 322 commits behind `origin/main`.

Workers are not affected by a stale root: they branch from a fetched `origin/main` (§ 7 rule 2) and read the contract at that commit (#1479). People, the PM and sessions opened in the root are affected.

## Classes

| Class | Paths | Tidy may |
|---|---|---|
| Task | `.worktrees/taskq-<N>` (worker, branch `taskq-<N>`), `.worktrees/review-<N>` (reviewer, detached at the result SHA) | remove when clean and the task left that stage |
| Reserved | `.worktrees/dev-build` (`scripts/dev-install.sh`) | nothing |
| Unowned | any other directory under `.worktrees` | remove when clean and untouched for 7 days |
| Foreign | registry entries outside `<root>/.worktrees`: Claude and Codex app worktrees, temp dirs, other checkouts | nothing; report only |

A reviewer that needs a checkout uses `.worktrees/review-<N>`; no other reviewer path.

## Tidy pass

Each root runs one tidy on the first service pass after start, then at most once an hour, and on `taskq cleanup` (which keeps `--force`). One root at a time; a root whose `.taskq` lock or Git lock is held skips the pass and says so.

1. `git worktree prune`: drops registry entries whose directory is gone. Locked entries stay (Git's own rule).
2. Task class: remove a clean worktree when its task is closed, or, for `review-<N>`, when the task is no longer in `review`. A live or unknown execution on this host keeps it (existing § 7 rule). Dirty: keep and report. Submodules: clean means clean in the worktree and in every checked-out submodule (`git status --ignore-submodules=none`); a submodule with commits on no remote keeps the worktree, also under `--force`, because those commits live only in the worktree's Git dir. Git refuses to remove any worktree with submodules, so a clean one is removed with `--force` after a fresh clean read. Each kept line names its reason: locked, uncommitted changes (and the submodule), submodule commits on no remote, or Git's refusal.
3. Unowned class: remove a clean worktree whose newest file and HEAD commit are older than 7 days. Its branch stays.
4. Branches: delete a local `taskq-<N>` with no worktree when the task is closed. `git branch -D` only when the close receipt records a merge (`merged #P as SHA`) or the tip is an ancestor of `origin/main`; otherwise `git branch -d`, and a refusal keeps the branch and reports it. No other branch is deleted.
5. Root: if HEAD is the branch `main`, run `git merge --ff-only origin/main` after the pass's `fetch`. Untracked or modified files that the merge does not touch stay as they are (the host's own `taskq.json` edits included). Divergence, a file the merge would overwrite, a detached HEAD, another branch or a Git operation in progress: no change, report `stale` with the reason and the behind count.

Never: `reset`, `stash`, `checkout`, `clean`, `worktree remove --force` (only `taskq cleanup --force`, which saves a patch of the worktree and of each submodule to `.taskq/trash`, or a clean worktree with submodules as in step 2), deleting an unmerged branch, or touching a foreign entry.

`workspace: external` projects skip steps 2 to 4 (host-owned worktrees, § 9); steps 1 and 5 still run.

## Report

The pass writes its last outcome to `<root>/.taskq/status.json` key `tidy`: time, root `current|behind N|stale <reason>`, and per class the count, dirty count, total size and oldest age. `taskq profile health` prints one line per root:

`<project>: root current|behind N|stale <reason>; .worktrees K (dirty D, S); foreign F (dirty G); tidy <age> <ok|skipped <reason>>`

The project log gets one line per removal, refusal and root move; a refusal line carries Git's first error line and repeats only when it changes; a pass that changes nothing logs nothing. Git runs with `core.longpaths` so a Windows removal of deep `target/` paths does not fail with `Filename too long`. Telemetry: one `service.tidy` operation per pass with the counts above, no paths. The PM periodic check reads the health line and flags dirty task worktrees older than a day, stale roots and size growth.

## Acceptance

- A closed task's clean worker and reviewer worktrees and its merged branch disappear within one hour on every host that has them; a dirty one stays and shows in health.
- An unowned clean worktree older than 7 days is removed, its branch kept; `dev-build` and foreign entries are never touched.
- A clean or unrelated-dirty root on `main` reaches `origin/main` within one hour; a diverged root, another branch or an overwrite conflict changes nothing and shows `stale <reason>`.
- No command in the pass can lose uncommitted work or unmerged commits (tests on a temp repo cover each refusal).
- macOS, Windows and Linux (FRA) roots behave the same.
