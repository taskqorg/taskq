# Mac 4.6.19 release and operator handoff

This is the #1233 version-PR handoff, not a deployment receipt. The owner accepted staged execution on #1233: normal worker/reviewer publication first, then an outside-service PM operator verifies assets and activates Mac. A2/A4 remain mandatory operator proof; #1244 must not enable publication policy before that proof. Keep queue focus open and unchanged. No Windows/FRA operations, profile changes, model/effort changes or new grants.

## Identity and retained evidence

- Version: `4.6.19`; Mac asset: `taskq-4.6.19-macos-aarch64` in release `v4.6.19` of `taskqorg/taskq`.
- Integration baseline: `be165eba211fef78465e83c21a4ee9afcdee9d5f` (#1232 / PR #1268), containing #1231 / PR #1261 and publisher #1243 / PR #1249 (`03a43228590f2d4867b6e35fdd2edb37acf1b199`).
- CoreQ 0.14.4: `5fb10926bbca0fdb042342dc5dc6ac3cc4e5a56f` in Cargo.toml and Cargo.lock; no dependency or runtime source changes in this version PR.
- The task result and PR head identify the immutable worker source. The reviewer publication receipt identifies the merge source; the release tag, exact-source tests and attestation must bind the actual published artifact to that source. A binary digest cannot be known before CI builds it; record it during verification below.
- Reuse [#1231 source/native boundary](qualification-1231.md) and [#1232 before/after proof](qualification-1232.md). The wake regression failed before repair and passed afterward; stale/unknown/unit barriers remain. Local Seatbelt checks were sandbox-blocked, and isolated launchd bootstrap exited 5: native retirement and installed progress remain unknown until operator proof. Do not replay the first qualification turn.

## Bounded outside-service procedure

Run only from the owner's PM/operator session, never this worker or its reviewer. Keep local paths, tokens, raw configuration and private backups out of public receipts. Commands below assume macOS arm64 and an existing authenticated host credential; never run `gh auth login`. If authentication is missing, stop for the owner.

1. Wait for normal reviewer publication. Record the merge receipt, immutable tag target and successful release run. Verify exact-source tests on that target and successful staged selftest in each published build; a green PR head alone does not establish merge-source CI. Poll at most once per minute. Missing/failed checks or absent assets stop activation.
2. In a new OS temporary directory, download the two named files and verify both attestations and the checksum:

   ```sh
   gh release download v4.6.19 --repo taskqorg/taskq --pattern taskq-4.6.19-macos-aarch64 --pattern SHA256SUMS
   gh attestation verify SHA256SUMS --repo taskqorg/taskq
   gh attestation verify taskq-4.6.19-macos-aarch64 --repo taskqorg/taskq
   shasum -a 256 taskq-4.6.19-macos-aarch64
   ```

   Compare the full digest with exactly one matching filename entry in SHA256SUMS. Verify attestation repository, workflow, tag and source commit against step 1. Retain that digest and source in the operator receipt. Do not execute an unverified download.
3. Inspect only the existing approved installation and its explicit projects list. Require exactly the six expected roots, readable host policy and native ownership. Save byte-for-byte, permission-preserving private originals of launcher/current/previous, host policy, projects, worker grants, lifecycle/install receipts, rollback floor and native unit definitions. Record existing work and history identities without exporting secrets. Preserve immutable release folders and migration backups. Conflicts or unknown ownership stop activation; do not discover extra roots or change focus/capacity/grants.
4. After verification, make the downloaded asset executable with `chmod u+x taskq-4.6.19-macos-aarch64`. Run the verified candidate's `selftest` from the existing TaskQ project, with offline mode unset; record its real board/telemetry outcomes. After confirming the updater still selects exactly 4.6.19 (stop if a newer release wins), use the supported installed CLI:

   ```sh
   /Users/kirs/.local/share/taskq/taskq update
   /Users/kirs/.local/share/taskq/taskq service status
   /Users/kirs/.local/share/taskq/taskq service list
   ```

   Read back current/previous and the installed candidate digest against step 2; current selection alone is not a running-artifact claim. Existing host topology uses normal re-exec and eligible CLI handover; active native turns may defer it. Legacy topology follows the approved [drain sequence](design/host-service.md#migration); never force-stop a live owner. No manual launchctl bootout, kill, fresh install or focus rewrite is part of this procedure.
5. Observe for at most 15 minutes, with readbacks at most once per minute. Require one `gui/<uid>/com.taskq.service` native unit, its launcher arguments/working directory, actual release executable PID/start identity and digest, and no remaining legacy owner. Every one of the six registered roots must have a fresh status naming the same host PID/generation and its own registration acknowledgement. Verify preserved project list, focus, grants, work and history. Require fresh board heartbeat/version, live selftest, readable capacity/admission, inventory-refresh progress for eligible work and the healthy marker after the configured rollback interval. Do not manufacture work or replay paid qualification. If no eligible work exists, queue progress is unknown, not PASS. A longer configured health interval or draining live work can exceed this observation window: report deferred/unknown and retain ownership rather than force completion.
6. Record the actual source/digest/version, native owner, six-root coverage, health, preserved state and any unknowns on #1233 through the authorized operator reporting path. Only this proof accepts deployment and releases #1244's gate. Failure to observe any required fact is not deployment acceptance.

## Failure and rollback

Stop on verification failure, changed updater target, stale state or uncertain ownership. Retain diagnostics and all original state. The observation window bounds the operator turn, not how long active work may drain.

The existing CoreQ launcher automatically selects a preserved compatible host release after three unhealthy starts and refuses versions below the recorded topology rollback floor. Verify the selected release, native owner and all six project statuses before reporting recovery. Do not overwrite releases, lower the floor, edit current behind a live owner or treat a saved backup as restore proof. The prior 4.6.18 is eligible only if its actual protocol and recorded floor permit it.

If automatic compatible rollback cannot recover, the operator requests the supported drain with `taskq service uninstall --host` only under the owner's rollback authority, then follows [reverse topology rollback](design/host-service.md#rollback). That command preserves projects and waits for verified idle/exit before native removal; unknown execution blocks it. Restoration of legacy units requires the separate owner decision specified there, saved matching originals and an update hold. This handoff does not authorize an automatic downgrade across the floor or manual restoration over live work. Retain worktrees, grants and history throughout; report rollback as incomplete until native and six-root readback passes.
