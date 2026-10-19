# Host-service integration handoff

Task #1231 integrates the prepared host-service candidate with current publication rules. This is a source handoff for #1232 and #1233, not production activation.

- Prepared TaskQ source: PR #1265, `506a1ae14fc02f1d2b722d6a973fbd504b2ee3aa` (4.6.18). Its full tree is retained by integration commit `94b3322`; PR #1261 and its earlier task-branch commits remain in history.
- Publication source: merged PR #1249 / task #1243, `03a43228590f2d4867b6e35fdd2edb37acf1b199`. Merge-group CI, queued publication, cancellation, private fallback and main-move retry semantics remain binding (§§2,7,8).
- Compatible dependency: CoreQ #55 / PR #56, merged `5fb10926bbca0fdb042342dc5dc6ac3cc4e5a56f`, version 0.14.4. Cargo, instrumentation and JavaScript use that immutable revision. CoreQ's reviewed source is unchanged from `8e6a9644976906271638569896b60949ed075ba3` except its CI workflow.
- Canonical replacement: the existing `taskq-1231` branch / PR #1261. Keep PR #1265 and its native evidence until review accepts the linked replacement. The TaskQ result receipt identifies the exact candidate SHA; it is not a release or activation receipt.

## Preserved protection

The prepared host, host installation/control, lifecycle, legacy recovery and telemetry implementations are reused unchanged. They retain single-host ownership, six-root operation, durable lifecycle receipts, execution fencing, bounded history, rollback-floor handling and Workspace enforcement. Existing source/process tests protect these boundaries. Publication adapters consume the prepared cached projection and continue pending publication without another reviewer.

The inherited update-lock regression uses main's pipe handshake instead of a fixed child delay; exclusivity, cleanup and inherited-descriptor assertions remain. The timer fixture keeps the prepared raw project-config isolation and both 60/900 assertions. No assertion is weakened to reconcile the candidates.

## Qualification boundary

PR #1265 records the prior six-project Mac Stop, one native unit/generation, Workspace boundaries, original-history import and healthy-marker evidence. This integration does not replay the paid first turn, activate a service, alter profiles/grants or operate another machine. Historical native evidence qualifies its recorded source/environment only (R12); it does not establish current deployed state or qualify the changed publication adapter. #1232 owns cutover qualification; #1233 owns release and operator activation handoff. Exact-candidate source checks and CI belong to the result/PR receipts; live merge-queue/settings qualification remains #1244.
