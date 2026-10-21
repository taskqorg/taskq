# Live panels before tasks — #1466

Implements TW3 from the approved #1443 plan under R6/R12 and § 7.
No rule, host policy, execution lifecycle or public asset API is amended.

Home reuses the executing-agent projection, work and workspace launchers, urgent
faults and the complete shared task list. Appearance follows the list; full
settings retain their purpose groups and shared navigation. Project identity
appears in the navigation once, with one independent Board link. Task rows retain
their title, issue, session and action boundaries. The original work-mode request
is available in Projects rather than occupying a separate Home Scope panel.

## Failure before, pass after

Base: `288c9ae59e98187c0aa28720c0e2b338acfc70d9`.
The new `node tests/tray-panels.browser.cjs` check failed on the base bundle:
Home had no complete task list; empty Attention occupied a row; project identity
and Board destination were repeated. The same checks pass after the change.
`tests/tray-running.test.cjs`, compiled against the base tray controller, failed
`duplicate execution must not double count`; it passes with the changed controller.
The identity includes board, task and execution, while workers/reviewers, runtime,
freshness, occupied slots and reservations remain distinct. Unknown observations
name their missing/stale/partial/conflicting source rather than showing zero.

The filter check now opens the project ancestor from the shared navigation,
instead of clicking a duplicate project identity inside the task filter. It
retains the same Back/OR filter assertion. No assertion was weakened.

## Visual evidence

The review set addresses the existing G05/G06 gaps only, with four images:

- [G05 before](G05-before.png) and [G05 after](G05-after.png): project identity,
  independent Board and full wrapped title.
- [G06 before](G06-before.png) and [G06 after](G06-after.png): empty Attention,
  actual executions and complete tasks on Home.

Chromium headless, invented fixture, 320 × 760 CSS px, dark 16 px. All four
images were opened individually and inspected at actual size: task text and
essential controls are visible without horizontal clipping. They replace the
G05/G06 observations in the #1443 review set; no new distinct gap is introduced.
No screenshot is a process-runtime or native-dispatch proof.

The component check protects runtime/role/host counts, stale and partial input,
reservation separation, escaping and execution deduplication. The browser check
protects Home composition, urgent faults, exact answer context, no policy/retry
writes and wrapped titles in light/dark at 16/20 px and 320 px. The existing
state-filter instrument protects all states, zero/unknown policy, OR, All tasks
project grouping, Reset, keyboard, refresh and Back across eight width/type/theme
cells. The shell route instrument covers all 13 tray route entries with fine and
coarse input, one navigation title, target size, focus and nonoverlap. Operation
and conversation browser checks retain workspace placement, immutable identity,
unknown-result fencing, drafts, IME and Send/Enter protection.

## Qualification limits

Owner accepted the G05/G06 actual-size layout in [answer 6328514474](https://github.com/taskqorg/taskq/issues/1466#issuecomment-6328514474). Native limits and merge_group CI remain open.
WKWebView/WebView2 clicks, window raise,
real effects and Windows native rendering are unverified. No production policy,
queue control, update, registration or failed-task retry was applied. Source and
browser checks cannot replace release-native qualification (R12).
Merge and green merge-result `merge_group` CI remain the publication gate (§ 7);
this worker does not wait for or claim that gate.

Final canonical gate, screenshot regeneration and publication receipts follow below.

## Final candidate checks

Implementation commit: `ed5aff69` (on base `288c9ae5`).
`sh docs/shots/check.sh` first exited 2 for the changed tray blob. After
`sh docs/shots/update.sh` built that immutable implementation, the same check
passed. The refreshed README project image and summary icon were opened and
inspected individually: one project identity/Board, readable task titles and
visible essential controls. The snapshot used an isolated installation and the
existing fixture; no running service was changed. The manifest records the
source blobs, binary hash and WKWebView environment. Home remains qualified by
the Chromium instrument only; native Home interaction is unverified.

- `node tests/tray-panels.browser.cjs`: passed, dark/light 16/20 px at 320 px.
- `node tests/tray-running.test.cjs`: passed, including deduplicated executions.
- `node tests/tray-state-filter.browser.cjs`: passed, eight theme/type/width cells.
- `node tests/shell-navigation.browser.cjs`: passed, 26 route/pointer cells;
  [text inventory](routes.json) records zero target/focus/name/overlap gaps.
- `node tests/tray-window.browser.cjs`: passed, both themes.
- `node tests/tray-view-state.browser.cjs`: passed.
- `node tests/workspace-communication.browser.cjs`: passed.
- `node tests/workspace-operations.browser.cjs`: passed; effects substituted.
- Navigation and workspace-launcher component checks passed.

- All 18 `tests/*.test.cjs` component scripts passed independently after the
  full gate stopped before reaching its Node stage.
- `python3 scripts/check.py --base 288c9ae59e98187c0aa28720c0e2b338acfc70d9`:
  **failed**, macOS arm64, new worktree/debug dependencies; 446 passed,
  18 failed, 34 ignored; 186.698 s / 900 s, no retries. The explicit host
  installation, policy and lifecycle worker refusals are **sandbox-blocked**,
  never PASS. Other assertion failures remain unqualified: legacy drain/exit
  proof, host acknowledgement/crash/readback and resource overlay expectations.
  No authority environment or protection was relaxed. Rust integration tests
  and canonical offline selftest were not reached; this is not a green gate.

Active distinct-gap review set: the six unaffected #1443 G01–G04/G07/G08
images plus the four G05/G06 before/after images here, ten images total. README
project/icon regeneration preserves the existing release screenshot assets and
adds no distinct-gap image to that set.

- Separate `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest`
  passed in an isolated temporary installation: 97 golden traces, formats and
  staged capture (3 committed observations). Board state remained explicitly
  unknown; the running installation/writer was not used.

## Owner acceptance and continuation

[Answer 6328514474](https://github.com/taskqorg/taskq/issues/1466#issuecomment-6328514474)
accepts the G05/G06 layout and directs result/review, leaving native limits and
merge_group CI open. Under R12/§ 7 the branch was rebased without conflicts onto
`4dfa99625f1c6f821e834c20762a48a66eb95f08`. UI blob IDs are unchanged; the accepted
images and before/after observations remain applicable. No new image or product
change was introduced by this continuation.

Dependencies changed on main, so the candidate checks were repeated:

- `python3 scripts/check.py --base 4dfa99625f1c6f821e834c20762a48a66eb95f08`:
  **failed**, 446 passed, 18 failed, 34 ignored; 58.499 s / 900 s on macOS arm64
  with warm dependencies, no retries. The explicit worker host installation,
  policy and lifecycle refusals remain **sandbox-blocked**, never PASS. Other
  host/resource assertions remain unqualified. Integration tests were not reached.
- All 18 Node component scripts and the seven browser instruments listed above
  passed against the rebased bundle, including the execution regression,
  G05/G06, filters, shell routes and workspace protections. Effects are substituted.
- `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest` passed in an
  isolated temporary installation: 97 traces, formats and staged capture with
  9 committed observations. Board read stayed explicitly unknown.
- `sh docs/shots/check.sh` passed with the unchanged UI blob IDs.

PR #1515 remains the single delivery PR. No merge, deployment, native-limit
qualification or merge_group CI success is claimed. The recorded owner layout
acceptance does not waive those limits or the publication gate.
