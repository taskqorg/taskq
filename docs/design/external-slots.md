# Local manager turn slots

## Contract detail

[taskq.md §3](../../taskq.md#local-external-consumer-slots-r1-r3-r9-r12) defines the authority (R1/R3/R9/R12). PilotQ #265 consumes this boundary; it does not create tasks or impersonate workers.

Minimum consumer API version: `taskq.slots/1`, advertised by a fresh `taskq service status --json` at `service.slots_protocol: 1`. The first release containing TaskQ #2100 is required; 4.6.57 without that change is unsupported. Check the advertised protocol, not a guessed release number. Ordinary released PDS/PilotQ integration is qualified by the PM; source fixtures and CI do not establish it.

## Call and readback

Run the installed `taskq` CLI as the existing host OS account, using its existing installation. The install/store permissions are the local trust boundary, as for owner CLI commands; `TASKQ_ATTEMPT` callers are refused. No enrollment, new grant, board write or service restart is needed. For a protected app-server launch, authenticated release additionally requires its host-verified native descendant completion/cleanup; unknown or legacy unproved completion refuses and retains occupancy. This grants capacity only. PilotQ remains responsible for its existing runtime permissions and process ownership.

1. Persist a unique `caller`/`op_id` and a cryptographically random 64-hex `key` for each turn before sending anything. Never reuse an ID for another turn. Keep the key private; it is release authority, not a board token. TaskQ stores its hash and never echoes the key.
2. Read the long-lived PilotQ owner's exact native identity and host boot with `taskq service slots --identity <PID> --json`. Save those fields with the operation. A restarted PilotQ reconciles the saved binding; it must not replace its PID or start another operation to resolve a lost response.
3. Submit `taskq service slots --action reserve --request '<JSON>' --json`, with the complete request below. Use the canonical, already registered project root, the actual turn runtime, and an admission deadline at most 300 seconds away. A board-only registered root is supported.
4. Read the same request with `--action observe`. Start runtime work only on `state: reserved` from a fresh matching host generation. `queued` and `busy` have no slot; `unknown` never authorizes a start. A lost response is reconciled with the identical request. PilotQ journals dispatch once per operation; a repeated `reserved` receipt never authorizes a second execution of the same turn. The CLI does not wait or schedule a retry; the existing host pass processes pending requests once per tick.
5. After success, failure or cancellation, stop/await the entire turn and every owned tool. Then send the same request with `--action release`. This is the authorized caller's completion attestation; it must never release while any of that turn still runs. Poll the same identity until `released`. Repeated reserve cannot reopen it. Release before admission cancels the request, even if its admission response was lost.

Illustrative request (invented key and identity; substitute every field):

```json
{
  "binding": {
    "schema": "taskq.slots/1",
    "caller": "pilotq",
    "op_id": "turn-unique-uuid",
    "root": "/approved/registered/project",
    "runtime": "codex",
    "pid": 12345,
    "started": "native-creation-identity",
    "boot": 1793490000,
    "expires_at": 1793490300
  },
  "key": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
}
```

Responses carry `schema`, `caller`, `op_id`, `state`, `occupied`, `reason`, host `instance` and observation `at` (Unix seconds); admitted turns also carry `granted_at`. A stale/missing host or mismatched generation yields `state: unknown` plus `stored_state`. `occupied: true` is retained occupancy, not permission to start. Input/authentication/storage refusals use the normal CLI error result. Policy, recovery, round-robin, spacing and load reasons are returned verbatim. External turns share host/project runtime counts and aggregate maximum; ordinary task focus stays unchanged and does not gate manager input.

## Recovery and bounds

Reserve/release and host admission use the existing locked, durable resource operation store; host grants share the worker/reviewer mutex. No consumer writes host.json or reconstructs capacity. All recovered external grants enter occupancy before any new grant; project sync cannot remove them. Eligible pending external roots join the existing project round-robin independently of task readiness. A task pass cannot clear their demand; admission, cancellation or denial removes it.

Admission expiry or a confirmed dead caller before admission denies a queued request. After admission, caller death, liveness errors, deadline expiry and service restart retain the slot; descendants might still run. The original key holder may reconcile and attest completion after checking its own native runtime ownership. A verified boot later than `granted_at` also proves the old turn ended. Missing boot/liveness stays unknown; time alone never frees a grant. TaskQ never kills a consumer process.

Release is durable before removal and observed only through host readback. Interrupted/corrupt store writes hold new host admission; files and existing occupancy stay intact. An in-memory grant missing from the store also fences admission. Losing the release key may retain a slot until verified reboot; protect PilotQ's operation journal. This deliberately preserves unknown execution rather than guessing that its tools exited.

The existing operation store holds at most 4096 bindings. Terminal slot receipts remain for 24 hours after termination and are pruned by the host pass; active/unknown reservations are never age-pruned. A full store refuses new identities while existing release/reconciliation remains available. Reserve, observe and release calls use the bounded resource lock wait; a busy store returns a retry/readback reason. No production installation, host policy edit or PDS integration is part of worker evidence.
