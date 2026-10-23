# Shared PM request candidate

Checked source candidate: `8b0ed84105633d07f0819defc897ffd96dd907bb`, rebased on `2c95bcf5315a9240691f4742830ff5e204196c46`. Windows GNU Rust 1.99.0, warm local build cache. This is producer/component evidence, not deployed FRA or native PM-session qualification.

## Applied contract

- R8/R13: amended §4/§5 and the linked PM/enrollment contract after [owner answer 1](https://github.com/taskqorg/taskq/issues/1615#issuecomment-6350767543). Existing owner-session detail remains in its canonical page; the root stays within 300 lines/60 KiB.
- R9: separate owner-created PM grants; observation/resource grants cannot launch a PM session. No live grants, runtime permissions, admission settings or installations changed.
- §7/R12: isolated task branch; bounded source/component/process checks. Native launch uncertainty keeps its original identity. Model/wire fixtures do not prove delivery to a real host.

## Verification

| Boundary | Observation |
|---|---|
| Canonical selected gate | `python3 scripts/check.py --base 2c95bcf5315a9240691f4742830ff5e204196c46`: **sandbox-blocked**, 485 Rust passes, 18 host-control worker-authority refusals, 34 ignored; final run 55.500 s local, not CI timing qualification. No worker authority was removed or bypassed. |
| PM dispatch and privacy | All five `pm::tests` pass inside that run. Shared fixture: accepted/native session, duplicate, denied, stale, expired and interrupted. Added connection replacement, narrowed grants, malformed runtime/grant state, retained durable prepare and independent local-request recovery. Real captured PM method observations contain no owner text; outgoing exchange and PM metadata also exclude it. Native launcher and wire are substituted. |
| Available process suite | `cargo +stable test --locked --test integration`: 17 pass, 1 ignored, including its telemetry-process module. There is no separate `telemetry_process` Cargo target in this checkout. |
| PM CLI | Isolated `cargo +stable run --locked -- service pm` reports action/version; a PM grant command from this worker is refused before creating PM grant state. |
| Component/IPC | All 21 `tests/*.test.cjs` pass. PM check additionally validates the built standalone bundle, exact receipt correlation, safe links, source hashes and release asset wiring. |
| Browser | `node tests/pm-composer.browser.cjs` and `node tests/conversations.browser.cjs` pass: send/outage/reopen/readback, failed persistence, unrelated stored receipt, existing CW1–CW8/attachment/IME/keyboard and native-only handoff behavior. Browser fixtures substitute native receipts. |
| Offline source selftest | Isolated `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest`: 97 golden traces and format checks pass; 9 staged capture observations committed. Board remains unknown/offline. |
| README native snapshots | `sh docs/shots/update.sh` and `sh docs/shots/check.sh` pass on Windows WebView2; manifest records source `1e64750d2fd401af623beb55a73e1b899f9590c1` and binary SHA-256. Project before/after was inspected: the version footer is now 4.6.32. Icon values remain unchanged. Owner visual acceptance is separate. |
| Snapshot-script protection | `sh docs/shots/check.test.sh` passes its fresh/stale/malformed/missing cases with the shared composer in the UI source inventory. |
| Release protection | `sh .github/release-tag.test.sh` and `python3 .github/release-check.test.py` pass; version/source/merge-queue release guards remain unchanged. |

Before/after defects caught during implementation:

- Failed draft persistence exposed Check delivery for unsaved input: browser assertion failed `1 !== 0`; after committing the in-memory request only after save, the same assertion passes. Immutable IDs and native delivery readback remain protected.
- The initial privacy probe did not observe PM method capture. It now enables CoreQ's supported diagnostic budget, clones the collector outside the dispatcher callback, flushes it, requires an observed `pm.exchange`, and checks owner-text absence. That test passes; capture is never assumed from an empty sink.
- Adding PM documentation exceeded the root contract's 60 KiB gate. Linking existing owner-session details and the dedicated PM contract restored the passing size gate without relaxing it.
- The rebased gate also exposed a cancellation-fixture panic on peer closure during a deadline case. Deliberately stalling initialization reproduced a fixture I/O timeout. The same focused check now passes and the final gate retains it: closure/I/O timeout is accepted only in deadline cases, with client deadlines, ownership, invalid-response rejection and no-replay assertions unchanged. Production cancellation code did not change.

## Artifact and open qualification

Candidate standalone bundle SHA-256: `8718133305abe624efa57aebac2b4d734ec60fd45d833a7f4df5ec368d4682f8`. Versioned JS/CSS/fixtures/source-manifest assets are wired into the release's `SHA256SUMS`; no new release was published. [Synthetic component image](pm-native-handoff.png) shows invented identities only.

The un-staged local GNU executable failed direct startup with `0xc0000135`; Cargo-driven native snapshots and offline selftest passed. This does not qualify an installed/staged product artifact or diagnose its missing dependency. Official release/platform qualification remains separate.

Per the [owner's answer](https://github.com/taskqorg/taskq/issues/1615#issuecomment-6354797682), this producer candidate is delivered on offline evidence. A5's real test-board request/readback moves to [dashq#195](https://github.com/taskqorg/dashq/issues/195), the held owner-authorized test-board qualification. A5 remains unproved here; no real native PM request was sent. DashQ route adoption/FRA end-to-end proof belongs with its consumer qualification; fixture success does not replace it. Merge and green `merge_group` CI remain required under §7/R12. Native accessibility/window behavior and other platforms were not tested here.
