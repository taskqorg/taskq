# TaskQ 4.6.60 release handoff

Prepare the patch release for [#2223](https://github.com/taskqorg/taskq/issues/2223), so v4.6.59 hosts receive the admission fix for stuck ready tasks such as #2188 and Windows scheduling correction. The PM owns publication and installed verification. This worker changes version metadata, changelog and release evidence only; live publication, tagging, installation and service restart are outside its scope (sections 7-8).

## Source and range

Preparation readback on 2026-11-03:

- Latest published release: [v4.6.59](https://github.com/taskqorg/taskq/releases/tag/v4.6.59), published 2026-11-02T15:20:33Z.
- Remote `live` and `v4.6.59`: `a0c8d298bf8a8d9ee7987e4953b6524fca5f63ea`.
- `origin/main`: `5ed011e86552daf5e1bfa68d3059f3a1366036b6`; Cargo package version 4.6.59.
- No v4.6.60 tag or competing open version PR was listed. Next unused patch: 4.6.60. Recheck before publication and stop if consumed; never replace tags or assets.

The complete `v4.6.59..5ed011e86552daf5e1bfa68d3059f3a1366036b6` range contains 12 merges, oldest first:

| Merge | Task / PR | Included behavior |
| --- | --- | --- |
| `74d0138a58b4a52363a6d7685df75f18d6fcb7a6` | #2176 / #2191 | Conversations projects AgentQ normalized history, tool results, lifecycle and per-runtime capability. |
| `46f6a560e823554a01e15816b3c4a72afdb49e6a` | #2202 / #2206 | Rule approval reads the leading N.K pick in a noted answer. |
| `64388400c881dcc88f5221bd2187b3726e9f4c88` | #2201 / #2205 | Spillover ignores untrusted host heartbeats. |
| `eaabcd70cdfe2bb064370af4a96a75f15e1d6c08` | #2198 / #2203 | Claude background-job turns continue in the same session with a bounded steer. |
| `0778be2fdf725f96a2aa886d4803f556e5b4b08d` | #2194 / #2199 | Doctor checks token permissions, versions, collection health and workers across hosts. |
| `f6d1a0fbb72defc23880a9052418caf0d2ca22ec` | #2209 / #2213 | macOS re-exec waits name themselves; Claude recovery distinguishes resumed jobs by input. |
| `d76b16d92bafdd4c9a3b12e88e899ec2d0b21703` | #2208 / #2211 | Failed dev-install logs retained per SHA. |
| `17a9960818df659c7133d60ec18d43eb7b372336` | #2184 / #2216 | Native A6 qualification report and evidence; documentation only. |
| `fdd2d92ba14c4eb46711a0489e34d8301ea5ed6b` | #2215 / #2218 | History instrument matches document-author answer format while retaining paging/read-only protections. |
| `1b953b914b360cf544cc91e330e086cc2fafdd4d` | #2214 / #2217 | Windows host restores normal CPU, I/O and memory scheduling when inherited lower. |
| `d64ad6c874215fa35f98c4c78be07dc5f959113b` | #2221 / #2222 | Requeued tasks admit after a resumed attempt hands in without a transcript match. |
| `5ed011e86552daf5e1bfa68d3059f3a1366036b6` | #2196, #2215 / #2219 | Service fixtures use isolated installs; fixed-clock progress and future-status fixtures retain their assertions. |

Tasks come from merge-body `Task:` lines; PR numbers come from merge subjects. The #2219 squash body also includes #2215; its tree changes are fixture isolation and timing corrections.

Dependency change in the range: `agentq-runtime` moves from `9be735b5a82f5a526cd8b1757196387956c3b5d0` to `ce1e27883a0e8e96ea02a9c3d241e39672d14f17`; CoreQ stays at v0.18.0. The version PR changes only the TaskQ package version in Cargo.toml/Cargo.lock, not dependency resolution.

## Compatibility and proof limits

Heartbeats add doctor health and request/answer observations (#2194); older hosts do not supply these checks and remain unknown. No task-log format, fold transition, label, credential location or open-issue migration changes. The 4.6.59 balancing cutover still applies: compatible readers on every board host precede weights or reroute. This patch enables no weights, grants, accounts or live policy (section 9).

The admission fix accepts an attempt's hand-in written after execution began without requiring a transcript match, and recognizes the canonical copied brief's input identity. It preserves execution fencing and unknown-state refusal. Windows scheduling correction raises only inherited lower priorities. The native scheduling instrument passed both profiles, so historical timeout causality remains unproved; see [Windows evidence](testing/windows-probes-2214.md). Source checks do not prove #2188 resumed on installed hosts (R12).

This release preparation adds no defect fix or test changes, so it claims no new failure-before/pass-after evidence. Existing fixes and their protections are inventoried above. The canonical candidate gate is `python3 scripts/check.py` after the one pre-push rebase (section 7 rule 4). Candidate merge-result CI, staged artifacts and owner live-host checks remain pending until their ordinary stages. Native history, lifecycle and UI evidence qualify only their recorded scopes (R8, R12).

## PM publication and readback

1. Let ordinary review publish the version PR through the required merge queue, without admin bypass. Read `gh pr view <version-pr> --repo taskqorg/taskq --json state,mergeCommit,url`. Record the full accepted merge SHA as `M`, not the worker branch SHA.
2. Fetch main, live and tags. Require `M` on main, containing `5ed011e86552daf5e1bfa68d3059f3a1366036b6`, with package version 4.6.60 in Cargo.toml/Cargo.lock. Inventory every commit in `v4.6.59..M`, adding merges after this readback. Stop on a concurrent version/release or non-fast-forward live; recheck latest release and v4.6.60 tag/assets read-only.
3. Require successful tests.yml **merge_group** CI on exact `M`, including Linux, macOS, Windows and Windows GNU. Read JSON; poll at most once per minute. PR CI is insufficient. Record the owner batch of affected live-host checks in the version PR and obtain the owner's release go (section 7 rule 8, R12).
4. With that go, PM fast-forwards live to exact `M` through section 8: `git push origin M:refs/heads/live`, substituting the full SHA, without force. This is the publication trigger. Do not manually tag or upload assets.
5. Read back `live == M`, `v4.6.60 == M` and the successful release workflow for `M`. It requires exact-source tests, locked builds and each staged binary's offline selftest from its isolated distribution directory. Failure leaves publication unverified; never overwrite a release.
6. Verify Linux/macOS/Windows assets, latest.json version/file map, SHA256SUMS and build attestations. README-shots dispatch creates its own draft PR; this version bump neither refreshes shots nor accepts native UI.
7. PM checks rollout in host heartbeats and DashQ, including desiign: each host reports 4.6.60 with installed executable provenance. Verify stuck ready work such as #2188 can admit when its ordinary gates permit; do not clear unresolved execution or change policy to force admission. A lagging host beyond one update interval warrants a scoped defect task.

Publication, accepted merge-result CI, artifact verification and installed rollout are unknown at worker preparation time. Reviewer records merge and queue CI; PM records publication and installed readbacks. Rollback uses the preserved previous version and retains board comments; the existing balancing compatibility requirement still binds.
