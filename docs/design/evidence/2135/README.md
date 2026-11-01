# Protected host request compatibility

Task: [#2135](https://github.com/taskqorg/taskq/issues/2135). Amended rule: R9, with the owning [supported-config rule](../../protected-app-server.md#contract-detail) updated before implementation (R13). The adapter remains under the existing R8/R9 outer Workspace boundary.

## Failure and retained receipts

The [actual Mac failure](https://github.com/taskqorg/taskq/issues/2131#issuecomment-6559239507) used `-s workspace-write -c sandbox_workspace_write.network_access=true`. Build/sign and existing-grant slot admission passed; the resolver refused the network config before native spawn. The original slot release was acknowledged. This is the compatibility regression, not a need for a new grant or host setting.

Original source/publication remain [PR #2132](https://github.com/taskqorg/taskq/pull/2132), reviewed source `deb1f66e2d97fad475623e03985c896a706d9b03`, merged source `8d75d224133cd18aba592a03031b0da597dcd28b`, and [green merge-result receipt](https://github.com/taskqorg/taskq/issues/2131#issuecomment-6559044651). The failed staged Mac binary was SHA-256 `bff5934da3ba639f3e49381b690a38bd6ba8e1d6b132f6a4fe269a1f946b86d2`. The [original evidence](../2131/README.md) remains unchanged; its earlier native gaps are not overwritten by portable results here.

With the new regression and unchanged production code at that base, `cargo test --locked --bin taskq protected_workspace_network_host_request_retains_authority_and_config -- --nocapture` failed: `existing host Workspace/network request must resolve: unsupported host app-server config; existing paths unchanged` (0 passed, 1 failed). The test uses the actual four host arguments through the adapter's existing `resolve` boundary with an invented registered project and operator grant, not native Mac execution. The cold Windows build took 8m21s; the failed test took 0.04s. Fixture children use the existing `scripts.check.test_environment` isolation; the launching worker's own task binding stays intact.

## Protection and consumer handoff

After the one-line allowlist correction, `cargo test --locked --bin taskq protected_ -- --nocapture` passed all 8 selected checks on Windows (1.08s test execution), including the same regression and the retained authority/input-race, tampering, slot, cancellation, unknown-liveness, release-retention and no-replay checks. No existing guarding assertion changed. This is portable resolver/state evidence, not native containment qualification. The frozen-candidate canonical gate and immutable source/artifact/CI receipts are reported on the task and PR; missing receipts remain unknown under R12.

Only the exact network-enabled config joins the existing model/effort allowlist. It is preserved in the native arguments and authority receipt; TaskQ still resolves the operator's explicit project Workspace grant and disables the inner sandbox only inside the required outer profile. `false`, malformed/unknown config, alternate sandbox modes and unknown flags refuse. No authority resolver is added to AgentQ.

The regression exercises the actual host shape, long option aliases with model/effort, unchanged outer profile and account home, and unsupported-config refusal. Existing authority, revocation/input locking, receipt/native/supervisor tampering, host generation, slot ownership, cancellation and interrupted-launch/no-replay assertions remain unchanged.

[AgentQ #186](https://github.com/taskqorg/agentq/issues/186) retains its active writer and optional transport API. Follow the existing [consumer recipe](../../protected-app-server.md#consumer-recipe): reserve/observe the original consumer slot, launch the corrected immutable TaskQ candidate without extra native flags, require separate fresh verified-effective authority, and reconcile the whole turn before release. Never substitute a copied receipt, retry an uncertain launch, change host config or regenerate the retained AssetQ crate.

## Native gate

PM must build/sign and hash the exact corrected source on Mac, then follow the existing [bounded qualification sequence](../../protected-app-server.md#qualification) using the existing AssetQ Workspace grant and host slot. Record the corrected source/tree, staged TaskQ and native Codex SHA-256, effective authority and exact native identity, and non-generating initialize/initialized result. The actual host-config failure must resolve there. Record filesystem/network/child and protected control/outside write/delete/rename results separately, including any missing scope. Verify exact native/probe completion before original-slot release acknowledgment; unknown execution retains occupancy.

Windows regression/selftest artifacts do not qualify Mac build, native execution, handshake or containment (R12). No model turn, paid call, ordinary installation, production interruption, forced slot release, or grant/model/account/capacity/config change is authorized by this handoff.
