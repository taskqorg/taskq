# Task completion

## Contract detail

These §7 rules bind under [the taskq contract](../../taskq.md#7-worker-reviewer-publication), R8 and R13. Section references below name that root contract.

### Evidence-only completion (R5, R13)
An owner may authorize a non-code evidence outcome with `taskq evidence N --sha SHA --baseline SHA --url URL --sha256 HASH --revision R --op-id ID --readers-current yes [--source-record NAME | --source-url URL --source-sha256 HASH]`.
This is separate from source publication: it neither merges nor publishes the candidate.
Authorization binds the board/task, current result entry and exact candidate, attempt baseline, task text/type, evidence URL/content hash and current revision.
The owner confirms all board readers support this route before authorizing; mixed readers must not reconcile it (§8).
A trusted immutable `taskq: evidence-authorized` audit records this binding; it does not change §2 fold.
Initial close and reconciliation verify the same authorization and fresh attachment bytes matching SHA-256.
No source delta needs a clean owned task worktree at its recorded branch-creation baseline, or an explicitly authorized archive source record for a checkout-free outcome.
`--source-record` binds an exact ZIP member with one `source <SHA> dirty= 0` line; its SHA must equal both candidate and baseline.
The record freezes the clean source observation, not new native qualification. An existing owned task worktree still must pass the workspace check; missing archive records refuse.
Separate archival provenance uses `--source-url URL --source-sha256 HASH` instead of `--source-record`: the owner binds a canonical source ZIP to the exact candidate, recorded attempt baseline, unchanged evidence hash and current task/result identity in the same authorization.
Verification freshly hashes both artifacts and compares every canonical ZIP file, path and byte with the candidate's Git archive; missing candidate objects or ambiguous/unsupported ZIPs refuse.
An existing owned worktree must be clean at its recorded baseline, with no task source delta; it is never reset to the candidate. Without a checkout, the explicitly authorized canonical ZIP supplies the frozen source proof. This does not qualify new native behavior.
Rule approval alone never authorizes evidence completion; the exact owner authorization and confirmation that every board reader supports separate archival provenance are required before use. PM verifies official reader rollout separately from worker local/CI evidence.
Missing, changed, ambiguous or unknown identity refuses before source publication.
Only research/docs/asset tasks qualify; code and unknown types retain ordinary publication CI, including merge_group or private exact-SHA CI.
Required/stale owner gates still block.
A changed result, task scope, baseline or evidence needs fresh authorization.
Completion uses one revision-fenced `close` receipt naming authorization, candidate and evidence; lost responses/restart read back that receipt without native launch, input replay or source publication.
Existing ancestry-only completion remains supported.
Evidence completion proves no additional native acceptance.
Summary names evidence-only completion and links its receipt; source readiness stays unknown without source publication, and no deploy-waiting label is added.
### Result and close verdicts
`result` body: `Changed: <user effect>` (1-2 lines), `Checks: <command -- outcome>` (1-8 lines, one per check), `Open: <remaining item|none>` (1-2 lines); research adds `Report: docs/research/<N>.md` last; ≤ 12 lines, 240 characters each. Keep exact commands and platform qualifications (R12).
`--text`: literal Changed/Open, optional Done/Problem (§ 2), `--option`/`--recommend`/`--link`; absent: no card. `--checks` prefixes Checks. SHA: log header. Changed may link one PR/evidence with [risk](https://github.com/taskqorg/specq/blob/main/caveman/rules/changes.md#merge-request-description-markdown), either route. `close --text`: same limits/fields; Checks: each `A<k> ok|gap|unknown`, merge-result CI (queued: pending; private fallback: exact-SHA).
Outcome (events, notices, `--closed`): `delivered`, `no change` (Changed none) or `replaced by #M (<state>)` (`superseded by #M`), plus `N items unverified`. Closing for held #M starts it; an agent asks instead. Review Acceptance and contract. Checks follows [review](https://github.com/taskqorg/specq/blob/main/engineering/agent-workflow.md#review). Research/proposals need a report; decisions blocking another task require ask before close.
Requeue only Acceptance/contract gaps; advisories: Open. Close with verdict, requeue with fixes, or ask. PM relays verbatim, linked. `close`: board API, except private fallback Git. Routes `direct`, GitHub `pr`, private GitHub, GitLab and merged but open, no-commit closes, CI refusals and worktree removal: [publication routes][ab].
**Delivery presets** are § 9 `delivery` `{preset, revision}`, the owner's (R9; revision +1 per owner edit): `auto-merge` (the `pr` route), `direct`, `review-pr` (`pr` after the owner gate). `local` and `push-branch` are refused. Without `delivery`: `publish` preset, revision 0. Pending publications, the verdict's delivery receipt: [agent behavior][ab].

[ab]: agent-behavior.md#contract-detail
