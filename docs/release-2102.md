# TaskQ 4.6.58 release handoff

Prepare the ordinary patch release for [#2102](https://github.com/taskqorg/taskq/issues/2102). The PM owns publication and installed pds18 verification for PilotQ #266. This worker changes version metadata and release evidence only; it does not publish, install, restart services or accept native UI.

## Source and range

Fresh preparation readback on 2026-11-01:

- Latest published release: [v4.6.57](https://github.com/taskqorg/taskq/releases/tag/v4.6.57), published 2026-10-31T21:46:30Z.
- `live` and `v4.6.57`: `fb901a7de11d7b40e73e3d7567ef2486123fa4c5`.
- `main`: `4bbce5e26d6cab9f113c055bf15cc33516162edc`; Cargo package version 4.6.57.
- No v4.6.58 tag or release; no other open version PR was listed. The next unused patch is 4.6.58. Recheck before publication; stop if another release consumes it. Never replace tags or assets.

The complete `v4.6.57..4bbce5e26d6cab9f113c055bf15cc33516162edc` range contains three commits, in order:

| Merge | Task / PR | Included behavior |
| --- | --- | --- |
| `60c7868ce6d7a97247b65af718a1771605636cb1` | #2091 / #2092 | Report comment-read settlement and one-time healing of older cached proofs. |
| `e1382b5d85f58d6956d3ac28eb7320ecc16646b0` | #2093 / #2097 | Board-only PM commands; checkout refusal for service installation and cleanup; § 5 contract and regression check. |
| `4bbce5e26d6cab9f113c055bf15cc33516162edc` | #2095 / #2098 | Conversations centre, navigation and reading preservation; existing UI evidence and browser instruments; portable tidy-test transport refusal. |

`git merge-base --is-ancestor e1382b5d85f58d6956d3ac28eb7320ecc16646b0 HEAD` establishes exact inclusion. [Board-only merge-result CI](https://github.com/taskqorg/taskq/actions/runs/41275330221) and [base main merge-result CI](https://github.com/taskqorg/taskq/actions/runs/41277328788) completed successfully on those exact SHAs. They do not establish the version candidate's CI or an installed host result (R12).

The version PR adds the Cargo package bump and this release documentation to that range. Existing UI evidence under `docs/design/evidence/2095/` is retained; #2096 and #2100 and the README screenshot PRs remain separate work. Recompute the range at the accepted version merge: unrelated main merges may join through the normal queue and must be inventoried before promotion.

## Compatibility and proof limits

No entry kind, state transition, board label, token location, runtime policy or dependency changes in this range. Existing open issues need no migration (§ 9). Report caches without the new settlement marker relist once; this is derived cache healing, not a board cutover. Older hosts retain their previous behavior until updated.

`src/main.rs` test `board_only_root_runs_pm_commands_and_refuses_checkout_work` uses a temporary root containing only `taskq.json`, an invented token in a separate temporary install and the in-process fake board. It runs status, inbox, issue and all seven PM writes, checks state/readbacks, permits only `.taskq` local output, refuses cleanup and service install, and verifies that no project was registered. The CLI renders an Error line naming the missing checkout, followed by Next (§ 5).

This is a source/command test: it does not exercise real GitHub HTTP, token authentication, installed root discovery or pds18. The fake board does not read the token. The spawn protection is the service-install guard; a previously registered root that later loses `.git` is not proven refused. No stronger worker-spawn claim is made. The release adds no feature fix, so no new defect before/after result is claimed.

The candidate's canonical gate is `python3 scripts/check.py` after its one pre-push rebase (§ 7 rule 4). Cargo metadata selects the full suite, including the existing board-only check, integration tests and isolated offline selftest. Results belong to the version PR and task result. Required merge-result CI and staged release-artifact checks remain pending until their ordinary workflow stages; native UI and installed acceptance remain separate (R12).

## PM publication and readback

1. Let ordinary review publish the version PR through the required merge queue. No admin bypass or auto-merge flag. Read its `state`, `mergeCommit.oid` and URL with `gh pr view <version-pr> --repo taskqorg/taskq --json state,mergeCommit,url`. Record the full accepted merge SHA as `M`; do not substitute the worker branch SHA.
2. Fetch `main`, `live` and tags. Confirm `M` is on main, contains the exact #2093 merge, and has Cargo.toml/Cargo.lock package version 4.6.58. Inventory every commit in `v4.6.57..M`. Stop on a concurrent version/release or a non-fast-forward live branch. Recheck the latest release, v4.6.58 tag/assets and live ref without changing them.
3. Require successful `tests.yml` **merge_group** CI on exact `M`, including Linux, macOS, Windows and Windows GNU checks. Read runs as JSON; poll no more than once per minute. PR CI or a main cache run is insufficient. Record the owner batch of affected live-host checks in the version PR and obtain the owner's release go (§ 7 rule 8, R12).
4. With that go, the PM fast-forwards `live` to exact `M` through the normal § 8 route (`git push origin M:refs/heads/live`, replacing `M` with the full accepted SHA; no force). That is the publication trigger. The worker does not perform it. Do not manually create a tag or upload assets.
5. Read back `live == M`, `v4.6.58 == M`, and the successful release workflow for `M`. It requires exact-source tests, builds with `--locked`, runs each staged binary's offline selftest from its isolated distribution directory, then publishes checksums, provenance, `latest.json` and platform binaries. A failure leaves publication unverified; never overwrite an existing release.
6. Verify the release metadata, expected Linux/macOS/Windows assets, `latest.json` version/file map, SHA256SUMS and build attestations. The automatic README-shots dispatch produces its own draft PR; this version bump neither refreshes shots nor accepts native UI.
7. PM verifies automatic rollout from heartbeats and DashQ. On pds18 read installed version and executable provenance, then run read-only status, inbox and issue from the approved board-only root with the host token, preserving the no-checkout layout. Record actual output and source/version. Do not create tasks or run worker setup/cleanup there to test refusal; their source regression is separate. Only this installed readback can release PilotQ #266's installed-version dependency. No CSGO checkout, host grant or runtime-policy change is part of this release.

Publication, release/platform CI, the exact accepted merge and pds18 remain unknown at worker preparation time. The reviewer records the accepted merge and queue CI; the PM records publication and installed readbacks. Rollback follows the existing preserved-version procedure; it does not remove board comments or report decisions.
