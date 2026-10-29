# Host link

Accepted implementation target under [taskq.md § 4 Host link](../../taskq.md#host-link-1832) (#1832). It lets the owner's Conversations panel show a live worker session that runs on another registered host of the same board ([live worker panel](ai-workspace.md#workspace-dialogue-d1d8)). Code: `src/link.rs`; the request path is `conversations::worker_read` and `conversations::serve`.

## Contract detail

**Transport.** DashQ is a blind relay ([dashq#234](https://github.com/taskqorg/dashq/issues/234), dashq 0.18.0): `POST <destination>/link/v1/send` with `{"protocol": 1, to, id, body}` and `POST <destination>/link/v1/recv` with `{"protocol": 1, "wait": 25}` (long poll; `{"protocol": 1, "messages": [{from, id, body}]}`, empty on timeout). The destination and bearer credential are the active [DashQ connection](enrollment.md) of this host; `to` and `from` are installation ids of one workspace. The relay keeps messages in memory only for 60 s, never reads a body, and limits each installation (5 per second, burst 20, 4 MiB queued); a newer `recv` of the same installation takes over and the older one returns empty. A refusal `{"protocol": 1, "error": <code>}` (`unsupported_protocol`, `scope`, `rate_limited`, `cap`, ...) is logged with its code and shows `unknown` on the viewer. TaskQ calls it with the OS `curl` like its other HTTP calls: URL, credential and body on stdin, never in argv.

**Identity (principle 1).** Each installation keeps one X25519 key pair in `<install>/link-key` (64 hex of the secret, created once like `installation-id`; a corrupt file is an error and stays untouched; a lost file is a new key). The install sweep keeps `link-key` and the reply spool `link/`. The service of the root that speaks for the DashQ connection (its board equals the approved scope) adds `link <installation-id> <public key hex>` to its heartbeat. Only trusted heartbeats carry a key into `status.json` `hosts[].link`.

**Sealing.** Every relay body is `base64(1 ‖ sender public key ‖ 32-byte random salt ‖ ChaCha20-Poly1305 ciphertext and tag)`. The key is HKDF-SHA256 over the X25519 shared secret, salted by the message salt, with info `taskq host link 1 ‖ sender key ‖ recipient key`; the version byte is the associated data. A fresh salt makes every message key new, so the nonce is zero. An all-zero shared secret (low-order key) is refused. X25519 follows RFC 7748 in `src/link.rs` and is checked against the RFC vectors and ring's ephemeral X25519; HKDF and the AEAD come from ring, so no crate is added. The relay sees sizes, timing and installation ids, never text.

**Who may talk (principle 2).** A host answers a `read` only when the sealed sender key and the relay's `from` both match one live peer: a trusted heartbeat of the project's board no older than `ttl` at the last board read. `taskq.json` `link_hosts` (host names) narrows the peers; absent, every live host of the board is a peer. A peer reads only that project's board: a request naming another registered project of the host is `unknown`. Anything else is dropped; the service log names the reason when it changes.

**Pull only (principles 3, 5, 7).** A request names `board`, `task`, `attempt` and one D4 cursor (`before` or `after`). The answering host finds the registered project serving that board with a fresh, complete status, and runs the same `worker_page` as a local read: the attempt must be the live worker attempt on that host with a running execution, and the session comes from the service projection and attempt log, never from the request. The reply is one D4 page of whole records. Over 180 KiB of page JSON, records are kept from the cursor side and the far cursor moves to the first record left out; one record over the bound keeps its items with shortened text marked `truncated`. The viewer sets the pace: one request at a time, one second apart. The link carries only these read-only pages; board state, answers and commands stay on the board.

**No state (principle 4).** Each viewer request has a random 128-bit id. The viewer writes `<install>/link/<id>.wait`, sends the request and waits; the host's own listener stores a `page` message as `<id>.reply` only while that marker exists. Both files are removed when the read ends, and the listener removes anything older than 60 s. A lost request or reply is not retried: the next one-second read asks again.

**Unknown (principle 6).** No reply within 5 s, no live key for the worker's host, no DashQ connection, a refused or unreadable reply, or an answering host that cannot read the worker is `unknown` with its reason, never an empty session.

**Listener.** The speaking project's service runs one long-poll thread per host (dashq#234). Errors back off from 5 to 300 s and log on change; the thread stops when the connection or the speaking role goes away.

## Qualification

| Case | Evidence | Open |
|---|---|---|
| Sealing | `link::tests::x25519_matches_rfc_7748_and_ring`, `sealed_bodies_open_only_for_their_host_and_name_the_sender` | — |
| A1 live items | fake relay: `a_read_crosses_the_fake_relay_sealed_and_fenced`; request shapes: `relay_requests_match_dashq_0_18`; browser `tests/worker-panel.browser.cjs` remote case; live relay round trip: `TASKQ_LINK_LIVE=<install> cargo test --locked --bin taskq link::tests::live_relay_round_trip -- --ignored --nocapture` (stop that install's service first: its listener takes over the receive) | live two-host check needs two enrolled hosts on this build: owner release batch (taskq.md § 7 rule 8) |
| A2 bounded, read-only, one attempt | `pages_fit_the_relay_bound_without_skipping_records`, `host_link_requests_serve_only_the_named_board_on_this_host`, `host_link_peers_read_only_the_listening_project_board`, `worker_reads_use_only_the_live_local_attempt` | — |
| A3 unknown | fake relay timeout; `worker_dialogue_refuses_stale_or_cross_project_reads` remote rows | — |
| Peers | `peers_are_live_heartbeats_of_the_board_and_the_allowlist`, `host_link_keys_come_only_from_trusted_heartbeats` | — |
