# TaskQ 4.6.59 release handoff

Prepare the ordinary patch release for [#2192](https://github.com/taskqorg/taskq/issues/2192), so hosts on v4.6.58 receive main, including #2189 balancing. The PM owns publication and installed verification. This worker changes version metadata, the changelog heading and release evidence only; it does not publish, tag, install, restart services or accept native UI.

## Source and range

Fresh preparation readback on 2026-11-02:

- Latest published release: [v4.6.58](https://github.com/taskqorg/taskq/releases/tag/v4.6.58), published 2026-11-01T06:41:33Z.
- `live` and `v4.6.58`: `dd695b8d1ad2fec03853ea4c6edff3d66faf7acb`.
- `main`: `b56633985b2c69f2a6a597c5335f20c36f704192`; Cargo package version 4.6.58.
- No v4.6.59 tag or release; no open version PR was listed. The next unused patch is 4.6.59. Recheck before publication; stop if another release consumes it. Never replace tags or assets.

The complete `v4.6.58..b56633985b2c69f2a6a597c5335f20c36f704192` range contains 37 squash merges, oldest first:

| Merge | Task / PR | Included behavior |
| --- | --- | --- |
| `fa89c4a00732ccd4ff24a9c085f93e5c03911195` | #2100 / #2103 | PilotQ turns share existing TaskQ host slots. |
| `d70f8a0e74178daaff18074c42202b076a2989b0` | #2099 / #2105 | Menu agent totals clarified; Held filter joins scoped admission. |
| `9e0806a48c8221d5a3b10a1f0033370f26e29542` | #2107 / #2109 | README three-part onboarding with repository board permission and optional SDD. |
| `643cf95335bf8e19eb631f645d7851f5f570256e` | #2118 / #2119 | Unpinned work falls back to eligible Codex when Claude is unavailable. |
| `a1a6b7c52a1ae1838b76a6ede2f76408de181812` | #2120 / #2126 | Windows fleet execution qualification report (docs only). |
| `8ed41aaf094a9f176d9b39e1edddd84eef08b73e` | #2121 / #2127 | Project attachment preflight before admission holds. |
| `9a1e2a818a3ed5bb9dc0bbbac275ebc2b42ab0c7` | #2094 / #2108 | TaskQ sessions run through the CoreQ agent; CLI versions reported. |
| `aebc4f167bed39228dab30a8c4e0a7358808ccdb` | #2111 / #2130 | Published AgentQ runtime adopted; native sessions preserved. |
| `f05783d08017f943b51ce8b2b32d1fb83ca19f2e` | #2125 / #2129 | Windows registry ownership preserved during project uninstall. |
| `476b207ea081389c181274a74ebc99d1d85f0da5` | #2110 / #2128 | Conversations bound reads and preserve reading context. |
| `8d75d224133cd18aba592a03031b0da597dcd28b` | #2131 / #2132 | Protected host app-server launch on existing external slots. |
| `fbc17b53acd043510ce0da4cad32433606ed5631` | #2133 / #2134 | Menu icon shows ready before questions and work. |
| `e6555153df9515974d7b5ec3c93af7cb5a9c481f` | #2135 / #2139 | Protected host request support; attributed sandbox refusals. |
| `0d050c51c2e1102c4b1a8d7af401f9beb362e54b` | #2136 / #2141 | Antigravity subscription workers and reviewers. |
| `a06aaaacbc3b61c85f45025633c887b939d382b8` | #2140 / #2142 | Conversations window recovery and clearer working history. |
| `e990d88943e95abd8d182d8157bfe84a94343a07` | #2145 / #2146 | Antigravity headless workers aligned with agy 1.4.12. |
| `521a2ae2f9eb9e522cb6fdde8e5e74327e115847` | #2143 / #2147 | Codex continuation input preserved after writer refusal. |
| `5f15bfa33a9a165c72de95f42f963cad92735b42` | #2137 / #2144 | Unified Balancing, soft task routing, owner `taskq reroute` fences and reviewer quota continuation. |
| `db318028d99c62c09a12884c337ce4d234a7ffb5` | #2138 / #2149 | Primary-first host spillover and collaborator runtime policies. |
| `015dd31a12d0bc0e31aaf7ef8db17a14100c0fee` | #2150 / #2152 | Antigravity shown permanently in the Running agents strip and table. |
| `07d261f2419d454662e2032d123bb22b68f1bf83` | #2153 / #2156 | Launcher stderr in `gone` reasons; account sandbox config seeded. |
| `ba7652694e5ab3aa343ad7f1767d04b0f7c50aa8` | #2155 / #2158 | Tray capacity review limits, workspace launcher and telemetry outcomes. |
| `2b8f14d8ae7c8986c8fb9021959ea0a37da3fbe1` | #2151 / #2154 | Protected authority preparation; native descendants reconciled. |
| `5292e63ffaeda41ca90d1b2e21b93f78fe2d3e39` | #2161 / #2162 | AgentQ optimization preserved across protected launchd scope. |
| `2feeed2c80b2cf9b1afe92413d7fb72af5bbdaa0` | #2160 / #2163 | GUI capacity save readback, project status poke and rejection recovery. |
| `26b9e6615f75aab6577c1663fdec5d8c77335bfa` | #2159 / #2164 | Conversations preserve PM context and native App replies. |
| `01bc4b66d93f2bd9a38ac4b5374c6a90242e4d86` | #2165 / #2170 | Manager launch choices remembered; effective access exposed. |
| `2eaae897fefd623beaa6ba920b45a494d2d122ff` | #2166 / #2169 | Claude default-home login and account isolation preserved. |
| `df078e69872d7b936d6dfd9d829d7553f1525fc5` | #2167 / #2172 | Workspace reader stamp dropped before tray capacity Apply. |
| `2dee3d5c6ada2204307b2cd28f9daa7dde270238` | #2175 / #2177 | Protected supervisor keeps certifying after frontend loss. |
| `4f42f8b44ead036461c0ff37b37910fe91d901db` | #2173 / #2174 | Task execution health exposed independently of host ownership. |
| `ebb9fd9264d3a310e86207b13aa749cd3fa8edcf` | #2157 / #2178 | Passive Codex App writer lock released before CLI worker resume. |
| `99c5e48a5076ee1aa1bd4cfda2a369249e152f1a` | #2179 / #2181 | Protected scope streams stay blocking; proven pre-native aborts certified. |
| `3d7fd541e6e30a578a0a83aeafe3d58d8f4c7b72` | #2166, #2168 / #2171 | Conversations feedback, runtime icons and times stabilized. |
| `01783bdebeaf0cf1d13d43bfb31cdd005442a8f0` | #2183 / #2187 | Registered CSGO Windows execution route report (docs only). |
| `50034cbaf6f09480c98732cf4b04ba951481c4ca` | #2189 / #2190 | Owner runtime weights and quota headroom for runtime and primary-host balancing. |
| `b56633985b2c69f2a6a597c5335f20c36f704192` | #2180 / #2185 | Native progress clock sampled after active logs; actionable stale heartbeats in manager status. |

Task numbers come from each merge's `Task:` trailers; #2155 and #2151 come from their merge subjects. [Base main merge-result CI](https://github.com/taskqorg/taskq/actions/runs/41459880039) completed successfully on exact `b5663398`. It does not establish the version candidate's CI or an installed host result (R12).

Dependencies changed in the range: `coreq` and `coreq-instrument` move from a pinned revision to tag `v0.18.0`; `agentq-runtime` (pinned revision `9be735b5`) and `imagesize` 0.15.0 are added. The version PR changes only the package version in Cargo.toml/Cargo.lock.

## Compatibility and proof limits

The changelog **Board** section names the board-visible changes:

- Weighted hosts publish heartbeat `balancing` schema 2; unweighted hosts keep schema 1. Weights stay off until the owner sets them, and need compatible readers on every board host first.
- Owner `taskq reroute` writes fenced `reroute`, `routing-write` and `rerouted` entries; v4.6.58 readers ignore them. Do not run `taskq reroute` until every board host runs 4.6.59.
- An updated host writes reviewer `steer quota attempt` after a local mid-review quota exit, without an extra gate. A v4.6.58 reader records it as a note but does not deliver a `steer` in `review` to the reviewer. Only the reviewer's own host acts on it, so the mixed-reader window lasts until the other hosts update; it is not qualified here.

No other entry kind, state transition, label, token location or open-issue migration changes (§ 9). Older hosts keep their previous behavior until they update. Automatic host updates and the [balancing cutover](design/configuration.md#balancing-cutover) are separate: this release enables no weights, reroute or live policy.

The release adds no feature fix of its own, so no new defect before/after result is claimed. The candidate's canonical gate is `python3 scripts/check.py` after its one pre-push rebase (§ 7 rule 4). Required merge-result CI and staged release-artifact checks remain pending until their ordinary workflow stages; native UI, protected app-server native readback and installed acceptance remain separate (R8, R12).

## PM publication and readback

1. Let ordinary review publish the version PR through the required merge queue. No admin bypass or auto-merge flag. Read its `state`, `mergeCommit.oid` and URL with `gh pr view <version-pr> --repo taskqorg/taskq --json state,mergeCommit,url`. Record the full accepted merge SHA as `M`; do not substitute the worker branch SHA.
2. Fetch `main`, `live` and tags. Confirm `M` is on main, contains `b56633985b2c69f2a6a597c5335f20c36f704192` (`git merge-base --is-ancestor`), and has Cargo.toml/Cargo.lock package version 4.6.59. Inventory every commit in `v4.6.58..M`; add any merge that joined after this readback. Stop on a concurrent version/release or a non-fast-forward live branch. Recheck the latest release, v4.6.59 tag/assets and live ref without changing them.
3. Require successful `tests.yml` **merge_group** CI on exact `M`, including Linux, macOS, Windows and Windows GNU checks. Read runs as JSON; poll no more than once per minute. PR CI or a main cache run is insufficient. Record the owner batch of affected live-host checks in the version PR and obtain the owner's release go (§ 7 rule 8, R12).
4. With that go, the PM fast-forwards `live` to exact `M` through the normal § 8 route (`git push origin M:refs/heads/live`, replacing `M` with the full accepted SHA; no force). That is the publication trigger. The worker does not perform it. Do not manually create a tag or upload assets.
5. Read back `live == M`, `v4.6.59 == M`, and the successful release workflow for `M`. It requires exact-source tests, builds with `--locked`, runs each staged binary's offline selftest from its isolated distribution directory, then publishes checksums, provenance, `latest.json` and platform binaries. A failure leaves publication unverified; never overwrite an existing release.
6. Verify the release metadata, expected Linux/macOS/Windows assets, `latest.json` version/file map, SHA256SUMS and build attestations. The automatic README-shots dispatch produces its own draft PR; this version bump neither refreshes shots nor accepts native UI.
7. PM verifies automatic rollout from heartbeats and DashQ: every board host, desiign included, reports 4.6.59. Read each host's installed version and executable provenance. Only after every host reads 4.6.59 may the owner consider weights or `taskq reroute` under the balancing cutover.

Publication, release/platform CI, the exact accepted merge and installed host versions remain unknown at worker preparation time. The reviewer records the accepted merge and queue CI; the PM records publication and installed readbacks. Rollback follows the existing preserved-version procedure; after routing or reviewer quota entries exist, rollback needs a compatible reader (balancing cutover). It never removes board comments.
