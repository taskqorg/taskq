# Protected app-server

## Contract detail

R8/R9: TaskQ supplies a narrow host adapter for external AgentQ/AssetQ Codex app-server consumers. Asset operations remain AssetQ jobs. This is an external consumer launch under §3, never a development-board attempt or resident authority service. Existing worker, reviewer and owner paths retain their APIs.

`taskq service app-server --action launch|observe|cancel --request JSON` accepts `taskq.app-server/1`. The request contains `schema`, an existing `slot` request (the `taskq.slots/1` binding and release key), `policy_revision`, and `account`. Unknown fields refuse. No executable, profile, permission, environment, model or grant text is accepted. The operation is the slot's immutable caller/op-id pair; one slot permits exactly one native launch, including after failure, interruption or host restart. The caller retains its original job identity and never replays uncertain work.

Launch is macOS Codex only, with a canonical registered project and explicit existing project Workspace grant. Host defaults, legacy and Full grants, Windows and other unknown enforcement refuse before native input. The adapter uses the existing policy resolver and runtime executable wrapper. It preserves host model/effort and the selected account home; it never changes login, native configuration or optimizer integration. Authority comes from the operator store, not task, environment, receipt or request text.

The host account's home comes from its native OS identity. Ambient `HOME`, `TASKQ_INSTALL` and `CODEX_HOME` must match the existing host locations. Native executable lookup uses the existing resolver against `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, `/bin` and the account's existing `.local/bin`, independent of consumer `PATH`. Custom installations and other account-home layouts remain unsupported by this adapter; existing consumers keep their paths. Receipts bind the resolved native image and digest, which consumers match to their qualified artifact. The generated profile additionally protects the adapter/native executable directories and standard host executable directories, including a staged candidate's directory.

This reuses the existing write boundary: its runtime homes, caches and temp exceptions remain; reads and network retain the existing profile's scope. It does not promise confinement of all reads, new network restrictions or isolation from an unrestricted same-user operator. No extra configuration or approval store is introduced.

As with existing Workspace launches, Codex's inner sandbox is disabled inside the required TaskQ Seatbelt; failed outer enforcement never retries native execution without that boundary. Launch and observe use the same environment for the resolver's protected-path set; a changed profile refuses. Occupied or unknown launch receipts retain their adapter release through normal update cleanup until the existing whole-turn slot release.

Supported host Codex arguments are `-s|--sandbox workspace-write|danger-full-access`, `-m|--model <model>`, and `-c|--config` with `model_reasoning_effort=<effort>` or exactly `sandbox_workspace_write.network_access=true`. The existing `-s workspace-write -c sandbox_workspace_write.network_access=true` request retains that config under the required outer Workspace boundary; it grants no authority. Model, effort and account remain unchanged. Network narrowing (`false`), other sandbox modes, unknown config and unknown arguments refuse rather than silently changing their meaning.

The host operation store retains launch intent on the existing slot before spawn. Its protected readback binds canonical project, generated profile digest, native executable path/digest, account/home, policy revision and approver, host generation, operation, supervisor and native PID/creation identities. Requested, saved and verified-effective facts are separate. Effective evidence names native image and effective sandbox queries; it does not claim exhaustive profile equivalence or native workload qualification.

Launch stdout is exclusively native app-server stdout. Native stderr remains stderr. Consumers read authority through a separate `observe` invocation of the trusted adapter. Neither a copied receipt nor a local receipt file authorizes input. Before forwarding native input, the supervisor verifies fresh matching slot authority, unchanged policy/account/executable/profile and exact native identity/enforcement, then commits and reads back its receipt. Every subsequent input delivery repeats authority validation. Refusal closes the input gate; no fallback or fresh identity retry occurs. Unknown process or store observations remain unknown.

Cancellation signals only the verified native incarnation. It never releases the slot or proves descendants/turn effects ended. EOF, caller loss, supervisor interruption, child exit and service restart do not attest tool completion. The external consumer retains slot ownership and must reconcile its entire native turn before submitting the existing irreversible slot release. Unknown work stays occupied under §3.

Cancellation is saved even before a native identity exists. A race with process creation may briefly start a child; the gate refuses its input and requests exact termination. A host-generation change, cancelled or unknown receipt, mismatched authority/native/supervisor identity, or changed native enforcement closes the gate. Exact cancellation requires live supervisor/child ownership; an orphan remains unknown rather than targeting a guessed PID.

## Consumer recipe

1. Reserve and observe an existing §3 slot for runtime `codex`, canonical project and the real consumer process identity. Keep its private release key and immutable job operation.
2. Read existing local `service permissions` and account state. Supply the exact approved policy revision/account; text is only a match requirement. No new grant or account is created.
3. Spawn the adapter's `launch` command with piped stdio. Do not append `app-server` or native flags. AgentQ #186 owns its reusable transport integration; existing `AppServer::start` consumers retain their compatible path.
4. Invoke `observe` separately with the same request. Require verified-effective readback bound to the expected project, account, executable and operation before sending an app-server handshake or turn. Never publish private roots, homes, release keys or credentials.
5. Use native thread/turn observation and interruption; `cancel` is process control only. Release the original slot only after the whole consumer turn and tools ended. An uncertain launch is observed, never relaunched.

## Qualification

Exact-source selected gate and affected merge-result CI are required (R12, §7). PM qualifies the same immutable staged candidate under the existing Mac AssetQ Workspace grant, with no ordinary installation of unmerged code. Record candidate source/build/artifact digest, trusted authority/native identity readbacks, non-generating app-server initialize/initialized exchange, approved filesystem/network/child probes and outside write/delete/rename and policy/launcher denials. Keep native receipts distinct from portable/fake checks and from deployed claims. No paid call, image generation or accepted-crate regeneration is authorized.

PM's bounded native sequence:

1. Build the exact candidate in an isolated checkout. Use the existing macOS signing runner (`cargo run`); hash the staged TaskQ and resolved native Codex executables. Do not install the candidate or alter `current`, grants, capacity, accounts or native configuration.
2. Run one real consumer process, reserve its existing host slot, and keep the release key private. Check the existing AssetQ project Workspace grant's current revision/approver and active account. Launch the candidate and observe its separate receipt; compare source/artifact, project, account/home, generated-profile hash and native/supervisor creation identities.
3. Perform only `initialize`/`initialized` and bounded non-generating native commands supported by that installed client's schema. Never start a model turn. Use an owned workspace fixture for positive read/write/rename/delete and a local loopback server for network readback; make a native command's child repeat the same boundary probes.
4. Prepare disposable canaries through PM in a dedicated protected host fixture outside the existing runtime write set. After kernel deny readback, attempt write/create/unlink/rename and a symlink alias against those canaries. All must be denied and their bytes/names unchanged. For real policy/launcher/account controls use nonmutating kernel queries; never attempt destructive operations on production controls or another project’s data.
5. Exercise native interruption, receipt tampering refusal and unknown liveness without replay. Stop and verify the exact owned native process and all probe children, then attest completion through the original slot release and observe host acknowledgment. If any execution is unknown, preserve the slot; no forced release or new operation identity.

Record every command/result and limitation in [task evidence](evidence/2131/README.md). Missing native API support, enforcement readback or real probe evidence is an open gate, never a portable PASS.
