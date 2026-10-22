# GitHub merge-queue recordings

Real GitHub responses that the merge-queue publication tests in `src/board/mod.rs` read through `recorded!`.
Each file holds one response body for one request that `src/board/github.rs` sends.
Recorded on taskqorg/taskq (private, merge queue on main) and octocat/Hello-World (public, no rules).
No tokens or private data: recorded with `gh api`, and only fields the code reads are kept in large REST lists.

| File | Request | Source |
|------|---------|--------|
| `queue-open.json` | `queue()` state query | PR #1574, open, never queued |
| `queue-enqueued.json` | `queue()` state query | PR #1590 while in the queue |
| `queue-merged.json` | `queue()` state query | PR #1589 after the queue merged it |
| `removal-merge-conflict.json`, `removal-failed-checks.json`, `removal-manual.json` | `timelineItems` part of the state query | PRs #1397, #1394, #1471: an earlier removal, read with `before:` the later merged event |
| `enqueue-conflict.json` | `enqueuePullRequest` refused | errors array from a taskq service log; `data` as GitHub sends it |
| `enqueue-already-queued.json` | `enqueuePullRequest` refused | PR #1590 while queued |
| `readback-enqueued.json`, `readback-none.json` | admission readback query | PRs #1590 and #1574 |
| `cancel-queued.json`, `cancel-open.json`, `cancel-merged.json` | `dequeue()` state query | PRs #1590, #1574, #1589 |
| `node-not-found.json` | any node query | invented node id |
| `ready-for-review.json` | `markPullRequestReadyForReview` | PR #1574 (already ready: no-op) |
| `rules-merge-queue.json`, `rules-none.json` | `GET /rules/branches/main` | taskq main; Hello-World master |
| `repo-public.json` | `GET /repos/{repo}` | Hello-World, reduced |
| `pulls-open.json` | `GET /pulls?head=...&state=open` | PR #1590, reduced |
| `runs-green.json` + `checks-green.json` | merge-result CI, green; push suite present | merge commit ab57340d (PR #1589) |
| `runs-failed.json` + `checks-failed.json` | merge_group run failed | merge group cc0cb967 |
| `runs-checks-queued.json` + `checks-queued.json` | run reported completed while its checks were still queued | merge group 635f9b16 |
| `runs-none.json` | no merge_group run yet | merge_group runs for a commit without one |
| `status-none.json` | `GET /commits/{sha}/status` | ab57340d: no commit statuses |

Assembled, not recorded: `enqueue-admitted.json` and `dequeue-done.json`.
A real success changes a live queue, so these follow the mutation's selection set with the recorded entry id.
Replace them with real bodies when a manual enqueue or dequeue happens anyway.

Tests overlay recordings only to join states no single PR shows: a removal timeline on the open PR, the PR list head set to the node's head,
the merge commit set to the SHA of the CI recording, and one push or merge_group check run changed to pending or failed.
Each overlay is named in the test.

## Re-record

Re-record when GitHub changes a field, an enum value or an error text, or when a test here disagrees with a live run.

```sh
id=$(gh api repos/taskqorg/taskq/pulls/<n> --jq .node_id)
# state query: copy the query string from queue() or dequeue() in src/board/github.rs
gh api graphql -f query='<query>' -f id="$id" > queue-<state>.json
# removal reason earlier than the last event: add before: <cursor of the later event> to timelineItems
gh api repos/taskqorg/taskq/rules/branches/main > rules-merge-queue.json
gh api 'repos/taskqorg/taskq/pulls?head=taskqorg:<branch>&base=main&state=open' \
  --jq '[.[]|{number,node_id,draft,state,head:{ref:.head.ref,sha:.head.sha},base:{ref:.base.ref},merged_at,merge_commit_sha}]' > pulls-open.json
gh api "repos/taskqorg/taskq/actions/runs?event=merge_group&head_sha=$sha&per_page=100" \
  --jq '{total_count,workflow_runs:[.workflow_runs[]|{id,name,event,head_sha,status,conclusion,check_suite_id}]}' > runs-<case>.json
gh api "repos/taskqorg/taskq/commits/$sha/check-runs?per_page=100" \
  --jq '{total_count,check_runs:[.check_runs[]|{id,name,head_sha,status,conclusion,app:{slug:.app.slug},check_suite:{id:.check_suite.id}}]}' > checks-<case>.json
gh api "repos/taskqorg/taskq/commits/$sha/status" --jq '{sha,state,total_count,statuses}' > status-none.json
```

Find a commit for each CI case with `gh api 'repos/taskqorg/taskq/actions/runs?event=merge_group&status=<failure|in_progress>'`.
Never run enqueue or dequeue against a PR only to record it: either one merges or drops real work.
Grep new files for tokens before committing, then run `cargo test --locked board::tests::github_`.
