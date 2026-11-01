# Protected app-server evidence

Task: [#2131](https://github.com/taskqorg/taskq/issues/2131). Base: `aebc4f167bed39228dab30a8c4e0a7358808ccdb`, after #2111 publication through #2130 with green merge-result CI. Amended rules: R8/R9. Protocol and bounded PM qualification: [protected app-server](../../protected-app-server.md#contract-detail).

## Before

- The installed `taskq service app-server --action observe --request '{}'` refuses with the existing service usage error; no supported protected app-server entry point exists.
- With the new regression and the original unmodified policy writer, `cargo test --locked --bin taskq protected_` fails `protected_grant_writer_shares_the_input_authority_lock`: a grant write succeeds while the native input authority lock is held. Protection retained: revocation and final native input serialize under the existing resource lock; no assertion was weakened.
- The first fresh-worktree compile failed because the required `esbuild` dependency was absent. The pinned `npm ci --ignore-scripts --no-audit --no-fund` setup completed through `npm.cmd`; no lifecycle scripts or host settings changed.
- That first direct focused run also inherited the worker attempt and correctly refused an owner-slot fixture. Subsequent checks use the repository's existing `test_environment` isolation; the worker's own binding remains intact.

## After

`cargo test --locked --bin taskq protected_` passed 6 focused cases on Windows after restoring the shared authority lock and using the existing isolated test environment. The initial Windows selected gate passed in 446.328s (822 unit and 20 integration cases, offline selftest and Node/packaging checks); digest-binding and release-retention corrections supersede that candidate, so a new frozen-candidate gate is required. Portable checks cover strict requests, explicit project Workspace authority, missing/corrupt/stale/duplicate grants, wrong account/path, unsupported OS, launch consumption, cancellation, host generation, receipt/native/supervisor tampering, unknown liveness, durable interruption and retained slot ownership. They do not qualify native Mac enforcement.

Final code candidate: `30872daf08ebb010e1300116f5a471febf7df7a4`; source tree `37c7bcb29fc21ec7db0cc33cb546d9037ea03a80`.

`python scripts/check.py --base f05783d08017f943b51ce8b2b32d1fb83ca19f2e` passed on Windows in 335.485s / 900s, with no retries: 823 unit cases passed (42 ignored), 20 integration cases passed (1 ignored), 113 fold traces, offline selftest, rule gate R8/R9, pinned contract lint, Node and packaging checks. The four new adapter/lock/retention checks and the new slot check passed in that gate. Read-only Astra review found no remaining binding defect. Only this evidence page was completed after the code gate; source/tests/dependencies remain identical.

Windows staged debug selftest artifact SHA-256: `18bc46c97cbbac6e653984cfe4f5debd536fbbc2e6c1a87b354624920950fd14`. This is a local Windows build checksum, not a Mac artifact, distributed release or native containment proof; the adapter refuses on Windows.

## Native and publication gates

Mac build/linking, the existing AssetQ Workspace grant, actual native image/sandbox readback, non-generating handshake, filesystem/network/child probes and outside write/delete/rename remain unverified. No Mac artifact or native receipt is claimed. PM must run the bounded sequence on the exact candidate; no ordinary installation of unmerged code is permitted.

Contract lint first refused the added R8 sentence at 442 characters; splitting it passed the pinned lint at 269 lines, 53361 bytes and longest line 394 characters.

Source/PR and affected merge-result CI receipts are added at publication. Candidate source, CI, staged artifact, native boundary qualification and deployment are separate claims. No installation, grant, capacity, account, login, accepted crate or asset job changed.
