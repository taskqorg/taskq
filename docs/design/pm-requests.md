# PM requests

Under taskq.md §4/R9, the shared PM composer sends one request and hands dialogue to the native runtime. The owner approved the separate opt-in exchange in #1615. This channel grants no task admission, runtime permission, resource control or native reply bridge.

## Contract detail

### Component and publication

`src/pm-composer.js` exports the composer and receipt used by Conversations and the standalone `TaskqPmComposer` browser bundle. Consumers use the release assets unchanged. The release publishes `taskq-V-pm-composer.js`, fixtures and source manifest under `SHA256SUMS`; the manifest pins UTF-8/LF source and fixture hashes. Version 1 uses schema `taskq.pm/1`. A delivery commit is a candidate until its release assets exist; DashQ adoption and live FRA qualification remain separate.

The adapter owns draft persistence and authenticated transport. Persist the exact request before Send. A timeout retains the request; Check delivery submits that same input, never a new op-id. A receipt must match actor, op-id and revision. Accepted means saved for launch; only a native session readback establishes handoff. Conversation replies stay native; the component has no chat backend or telemetry payload.

### Dispatch and authority

DashQ authenticates its actor and separately enables its PM route. The enrolled TaskQ service initiates HTTPS POST `/pm/v1/exchange` beneath the connection's `/machine` destination, using its existing connection credential. No inbound listener, SSH command, shell text or telemetry transport is added. Off without an unexpired host PM grant. A TaskQ host grant does not enable or configure DashQ.

`taskq service pm --json` lists registered project IDs/revisions, PM grants and receipts. The owner runs `service pm --grant ID --actor ACTOR --hours N --project PROJECT_ID --runtime RUNTIME` to authorize that actor, connection installation/generation, projects and supported owner runtimes for 1–720 hours. Replace/revoke is local owner-only. Resource/observation grants do not suffice. No grant widens runtime access. Recheck active connection, grant, project registration/revision, runtime and expiry before persisting dispatch.

Host POST: `{schema, installation_id, generation, targets, receipts}`. Each target names project ID, revision, runtimes and authorized grant/actor. Reply: `{schema, server_at, requests}`. At most 8 requests per exchange, 1 MiB reply, clock skew ≤30 seconds; request text ≤64 KiB UTF-8, no empty text. Request fields are `schema`, `actor_id`, `op_id`, `grant_id`, `revision`, `target` (`installation_id`, `generation`, `project_id`, `runtime_id`), `issued_at`, `expires_at`, `text`. Deadline ≤300 seconds after issue; admission must occur before expiry. No attachments in remote v1; local file handling remains #1580's.

Receipts echo actor/op/revision and name outcome (`accepted`, `rejected`, `stale`, `unknown`), request ID, native state, host, runtime, session and safe session URL when known. Rejections use fixed reasons. No request text, brief, paths or conversation content is returned in the exchange, logged or added to telemetry. Native setup owns the text/brief. Remote request IDs use a SHA-256 namespace, independent of tray IDs.

### Preservation and limits

`<install>/setup/pm/state` is governed owner-request metadata, under its own file lock. It stores grants, input hashes, dispatch intents and receipts, never a second task queue or conversation store. Persist an intent before setup creation; after any interrupted or uncertain write only read the same setup record. Missing/corrupt records remain unknown; do not recreate or launch. A repeated op-id with changed input is rejected. Keep tombstones for all admitted identities: 4096 operations and 64 grants maximum, no automatic deletion or ID reuse. At capacity refuse new input while existing receipts remain readable. Unavailable/corrupt state refuses changes and preserves bytes.

The service exchanges on a separate thread at most once per 5 seconds while active, 30 seconds idle, 60 seconds after failure. HTTP failures, denial, restart and lost receipts never replay native dispatch. Changed/revoked connections cannot receive old receipts. Local launch still uses §4 setup once-only recovery; no uncertain native launch is retried. Disabling the channel leaves existing native sessions alone.

### Verification

Shared fixtures cover typed input, denial/stale/duplicate/timeout, exact receipt correlation and native handoff. Rust dispatch tests use an isolated registered project and substitute the wire/native launcher; they prove adapter wiring, not FRA or native-runtime qualification. A real test-board request/readback and the released asset checksums must be reported separately under R12. Rollback disables PM exchange by revoking its grant, retaining setup identities and native sessions.
