# Merge queue rollout: merged workflows, activation held

This is partial evidence for #1244, not completed rollout or live qualification (R12).
The owner approved the six boards and reuse of #1243's amendments to sections 2, 7 and 8.
TaskQ main `03a43228590f2d4867b6e35fdd2edb37acf1b199` already contains those amendments, the queue adapter, fake-board publication checks, retry traces and `merge_group` workflow. The dirty primary checkout was preserved; no duplicate contract edit was needed (R13).

## Observed before

All six active main rulesets required strict head checks, with no bypass actors and no merge queue.
TaskQ's tests workflow already handled `merge_group`; the other five boards' required workflows did not.
DashQ requires both its test job and its release-build matrix, so both workflows need the event.
SpecQ's diff check also needs `merge_group.base_sha`; the PR/push base expressions are retained.
`workflow-changes.patch` contains the exact before/after event changes. No job, test or assertion was removed.
`workflow-prs.json` records immutable heads and PR links. These branches were created through `gh api` Git refs/trees/commits and pulls endpoints; no other local project root was accessed.

## Applied workflow state

- AgentQ: https://github.com/taskqorg/agentq/pull/157
- CoreQ: https://github.com/taskqorg/coreq/pull/54
- DashQ: https://github.com/taskqorg/dashq/pull/153
- ServerQ: https://github.com/taskqorg/serverq/pull/41
- SpecQ: https://github.com/taskqorg/specq/pull/69

All five PRs were squash-merged at their recorded heads after fresh green CI and mergeability readback. `workflow-merges.json` records the merge SHAs and main workflow blob SHAs. All six boards now have the required `merge_group` events on main; TaskQ already had them. No tests, jobs or assertions were removed. This proves workflow presence, not execution of a merge group (R12).
No repository settings were changed. `*.planned.json` are proposals, not readback evidence.
`*.rollback.json` preserve the original writable ruleset payloads.
The proposed rules retain every required check, deletion/force-push protection and empty bypass list.
Queue CI replaces strict head freshness. Each merge group is checked with ALLGREEN, squash publication, one PR per merge, two concurrent builds and a 60-minute check timeout.

## Resume and activation

Workflow merges and main event readback are complete. `rules-reconciliation.json` confirms all six rulesets still equal their saved original payloads. Recheck before applying; this comparison is not future authorization to overwrite concurrent changes.
Follow the approved publication cutover in taskq.md § 9 before exposing old services to queue-required boards; this attempt does not stop services, change host policy or deploy a binary.
Read each ruleset again and compare it to its rollback payload. A concurrent change requires reconciliation before PUT; never overwrite it blindly.
The intended activation commands, from this repository root, are:

```sh
gh api --method PUT repos/taskqorg/agentq/rulesets/25117820 --input docs/evidence/1244/agentq-rules.planned.json
gh api --method PUT repos/taskqorg/coreq/rulesets/25118262 --input docs/evidence/1244/coreq-rules.planned.json
gh api --method PUT repos/taskqorg/dashq/rulesets/25118356 --input docs/evidence/1244/dashq-rules.planned.json
gh api --method PUT repos/taskqorg/serverq/rulesets/25118437 --input docs/evidence/1244/serverq-rules.planned.json
gh api --method PUT repos/taskqorg/specq/rulesets/25118568 --input docs/evidence/1244/specq-rules.planned.json
gh api --method PUT repos/taskqorg/taskq/rulesets/25117578 --input docs/evidence/1244/taskq-rules.planned.json
```

After each PUT, GET the same endpoint and `/rules/branches/main`; verify the queue, required checks, protection and absence of bypass. A mutation response alone is not proof (R12).
The API shape follows [GitHub rules documentation](https://docs.github.com/en/rest/repos/rules#update-a-repository-ruleset).

## Remaining acceptance

A1 workflow merges are complete; activation and fresh readback remain incomplete on every board.
A2/A3 have implementation and regression protection from #1243; live queue behavior is unqualified here.
A4 still needs two PRs from two live tasks merging back to back without rebase requeues, correlated merge-result CI and task close events, plus release tag/publication evidence. No production release was triggered for this preparation.

To roll settings back, reconcile current state and PUT the matching `*.rollback.json`, then GET it back. Keep the event triggers while any PR is queued; removing them can strand CI. A rollback cannot undo an already merged PR or published release.

## Local verification on 03a43228

- `cargo test --locked`: exit 0; 246 unit and 6 integration tests passed, 2 ignored. The Seatbelt policy-store test reported `sandbox-blocked: Operation not permitted`; this is not a policy isolation PASS.
- `cargo run -- selftest`: exit 1; 82 golden traces and formats passed. Live board/telemetry access reported `Operation not permitted`; live selftest is sandbox-blocked, not PASS.
- `git diff --check`: passed.
- One fresh GET per workflow PR confirmed it was open, unmerged and at the recorded head. No CI wait was performed.

Before/after: five required workflow sets lacked merge_group before; their merged main blobs now contain it (`workflow-merges.json`). Live merge-group execution remains unproved. No tests or assertions were changed or weakened.

## Operator handoff: runtime activation remains blocked

`cutover-readback.json` records board observations after the workflow merges. FRA still publishes v4.5.0 heartbeats on AgentQ, DashQ, ServerQ and TaskQ. Windows last published v4.5.0; stale heartbeats do not prove its service or children stopped. Mac publishes v4.6.18 and runs this attempt. No host stop, upgrade, permission change, queue activation or Windows operation was performed in this attempt.

The blocking requirement is the approved publication cutover in taskq.md § 9, not workflow CI. Focus off does not prove stopped services or children. The worker must not stop its own live attempt (R11). An operator must coordinate the following after this turn ends:

1. Preserve each board's open tasks, results and pending publication receipts using fresh `taskq status --json` and `taskq inbox N` from its registered project. Limit boards to taskqorg/{agentq,coreq,dashq,serverq,specq,taskq}. Do not edit closed issues.
2. Inspect the owning native services and child IDs on Mac, FRA and Windows; a stale heartbeat is unknown. Resolve active work under R11 before stopping. Run `taskq service stop` in each owning installation and verify native process/child exit. This document does not authorize Windows work or removal of dirty worktrees.
3. With all old services and children confirmed stopped, re-read and reconcile the six rulesets. Apply the six exact PUT commands above, then GET each ruleset and `/rules/branches/main`. Require merge_queue, all retained checks, deletion/force-push protection and an empty bypass list. Save responses as activation evidence.
4. Upgrade the stopped installations to a verified released artifact containing #1249. Verify checksum/provenance and actual service version; configured `current` alone is insufficient. Preserve grants, model, effort and focus. Start Mac first, then Windows per § 9, and coordinate FRA before it resumes. Use `taskq service install` only within the operator's approved host scope; inspect its effects first.
5. Confirm fresh queue-capable heartbeats for every board host, then resume #1244 with `taskq requeue 1244` on TaskQ. Run two already authorized live tasks through review and `taskq close`; do not create synthetic owner work without authorization. Record both PRs, merge-group SHA/checks, merge receipts, close events and absence of main-move requeues. Record a real version PR's tag and published release; workflow presence is not release proof.

If host activation is outside the operator's current grant, obtain that exact grant; do not silently elevate or work on Windows. The task remains incomplete until activation and A4 are proved. Settings rollback is described above and cannot undo publication.

## Continuation verification

On the unchanged implementation at `03a43228590f2d4867b6e35fdd2edb37acf1b199`, `cargo test --locked` exited 0: 246 unit and 6 integration tests passed, 2 ignored. Seatbelt isolation remains sandbox-blocked. `cargo run -- selftest` exited 1: 82 traces and formats passed; live board/telemetry access is sandbox-blocked with `Operation not permitted`, not PASS. `git diff --check` passed. No code or tests changed; these documentation and remote workflow observations do not qualify A4. CI was not awaited.
