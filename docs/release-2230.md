# TaskQ 4.6.61 release handoff

Prepare the patch release for [#2230](https://github.com/taskqorg/taskq/issues/2230), so the Windows host leaves v4.6.59 through the update restart fix and admits ready tasks such as #2188. The PM owns publication and installed verification. This worker changes version metadata, changelog and release evidence; live publication, tagging, installation and service restart are outside its scope (sections 7-8).

## Source and range

Preparation readback on 2026-11-03:

- Latest published release: [v4.6.60](https://github.com/taskqorg/taskq/releases/tag/v4.6.60), published 2026-11-02T20:40:56Z.
- Remote `live` and `v4.6.60`: `3f6fbf70b171aa5f02bad7e5bf990048e91bef4a`.
- `origin/main`: `a7391cf8ab0a5b4cb3ec0310d92656e25a3557dc`; Cargo package version 4.6.60.
- No v4.6.61 tag or competing open version PR was listed. Next unused patch: 4.6.61. Recheck before publication and stop if consumed; never replace tags or assets.

The complete `v4.6.60..a7391cf8ab0a5b4cb3ec0310d92656e25a3557dc` range contains 2 merges, oldest first:

| Merge | Task / PR | Included behavior |
| --- | --- | --- |
| `601f268f61e76c41b98d829614fabd42090cac7c` | #2225 / #2227 | Public exports and peer transcript replies use background descriptions or fixed tool labels and omit tool result bodies and images. |
| `a7391cf8ab0a5b4cb3ec0310d92656e25a3557dc` | #2228 / #2229 | Windows image exit waits only for live children and app turns; an attempt with no observed process (such as #2188's handed-in Claude resume) no longer blocks the restart. Stop still waits. |

Tasks come from merge-body `Task:` lines; PR numbers come from merge subjects. No dependency changes in the range; the version PR changes only the TaskQ package version in Cargo.toml/Cargo.lock.

## Compatibility and proof limits

No task-log format, fold transition, label, heartbeat field, credential location or open-issue migration changes. The 4.6.59 balancing cutover still applies: compatible readers on every board host precede weights or reroute. This patch enables no weights, grants, accounts or live policy (section 9).

The restart fix lets the new image read the unresolved execution from the board, as #1435 does for childless observations; it preserves execution fencing and unknown-state refusal. The Windows host currently runs v4.6.59, so it reaches 4.6.61 through the old image's update path: the old image still carries the waiting exit. If the old image keeps waiting on #2188, the PM resolves it through the ordinary owner-confirmed host handover, never by clearing unresolved execution. Source checks do not prove #2188 resumed on installed hosts (R12).

The canonical candidate gate is `python3 scripts/check.py` after the one pre-push rebase (section 7 rule 4); its outcome is recorded in the version PR and task result. Candidate merge-result CI, staged artifacts and owner live-host checks remain pending until their ordinary stages (R8, R12).

## PM publication and readback

1. Let ordinary review publish the version PR through the required merge queue, without admin bypass. Read `gh pr view <version-pr> --repo taskqorg/taskq --json state,mergeCommit,url`. Record the full accepted merge SHA as `M`, not the worker branch SHA.
2. Fetch main, live and tags. Require `M` on main, containing `a7391cf8ab0a5b4cb3ec0310d92656e25a3557dc`, with package version 4.6.61 in Cargo.toml/Cargo.lock. Inventory every commit in `v4.6.60..M`, adding merges after this readback. Stop on a concurrent version/release or non-fast-forward live; recheck latest release and v4.6.61 tag/assets read-only.
3. Require successful tests.yml **merge_group** CI on exact `M`, including Linux, macOS, Windows and Windows GNU. Read JSON; poll at most once per minute. PR CI is insufficient. Record the owner batch of affected live-host checks in the version PR and obtain the owner's release go (section 7 rule 8, R12).
4. With that go, PM fast-forwards live to exact `M` through section 8: `git push origin M:refs/heads/live`, substituting the full SHA, without force. This is the publication trigger. Do not manually tag or upload assets.
5. Read back `live == M`, `v4.6.61 == M` and the successful release workflow for `M`. It requires exact-source tests, locked builds and each staged binary's offline selftest from its isolated distribution directory. Failure leaves publication unverified; never overwrite a release.
6. Verify Linux/macOS/Windows assets, latest.json version/file map, SHA256SUMS and build attestations. README-shots dispatch creates its own draft PR; this version bump neither refreshes shots nor accepts native UI.
7. PM checks rollout in host heartbeats and DashQ, including desiign: each host reports 4.6.61 with installed executable provenance, and win leaves v4.6.59. Verify stuck ready work such as #2188 can admit when its ordinary gates permit; do not clear unresolved execution or change policy to force admission. A lagging host beyond one update interval warrants a scoped defect task.

Publication, accepted merge-result CI, artifact verification and installed rollout are unknown at worker preparation time. Reviewer records merge and queue CI; PM records publication and installed readbacks. Rollback uses the preserved previous version and retains board comments; the existing balancing compatibility requirement still binds.
