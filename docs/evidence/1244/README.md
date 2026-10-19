# Merge queue rollout: staged workflows

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

## Staged state

- AgentQ: https://github.com/taskqorg/agentq/pull/157
- CoreQ: https://github.com/taskqorg/coreq/pull/54
- DashQ: https://github.com/taskqorg/dashq/pull/153
- ServerQ: https://github.com/taskqorg/serverq/pull/41
- SpecQ: https://github.com/taskqorg/specq/pull/69

These PRs were opened; CI was not awaited and their merge is not established here (§ 7.5).
No repository settings were changed. `*.planned.json` are proposals, not readback evidence.
`*.rollback.json` preserve the original writable ruleset payloads.
The proposed rules retain every required check, deletion/force-push protection and empty bypass list.
Queue CI replaces strict head freshness. Each merge group is checked with ALLGREEN, squash publication, one PR per merge, two concurrent builds and a 60-minute check timeout.

## Resume and activation

First verify all workflow PRs are merged and each required workflow on main handles `merge_group`.
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

A1 is incomplete until workflow merges, activation and fresh readback succeed on every board.
A2/A3 have implementation and regression protection from #1243; live queue behavior is unqualified here.
A4 still needs two PRs from two live tasks merging back to back without rebase requeues, correlated merge-result CI and task close events, plus release tag/publication evidence. No production release was triggered for this preparation.

To roll settings back, reconcile current state and PUT the matching `*.rollback.json`, then GET it back. Keep the event triggers while any PR is queued; removing them can strand CI. A rollback cannot undo an already merged PR or published release.

## Local verification on 03a43228

- `cargo test --locked`: exit 0; 246 unit and 6 integration tests passed, 2 ignored. The Seatbelt policy-store test reported `sandbox-blocked: Operation not permitted`; this is not a policy isolation PASS.
- `cargo run -- selftest`: exit 1; 82 golden traces and formats passed. Live board/telemetry access reported `Operation not permitted`; live selftest is sandbox-blocked, not PASS.
- `git diff --check`: passed.
- One fresh GET per workflow PR confirmed it was open, unmerged and at the recorded head. No CI wait was performed.

Before/after limitation: the remote before state is recorded and proposed event diffs are saved, but no applied-main after state or live merge proof exists yet. This preparation changes no tests and weakens no assertions.
