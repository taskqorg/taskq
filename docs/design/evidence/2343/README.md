# Owner UI review

Source: [all owner findings and start authorization](https://github.com/taskqorg/taskq/issues/2343). The retained notes supply the original observations, expected results and five screenshots. The named amendment and R6/R12 test-scope answers approve the UI rules and adapted assertions. They grant no new behavior policy, host configuration or live deployment.

## Specification checkpoint

`taskq.md` Product and its owning details were amended before implementation. Findings retain their owner IDs. The table maps the required work and its evidence boundary; it is not a passing acceptance report.

| Finding | Screen and requested change | Owning specification | Source screenshot | Verification boundary |
|---|---|---|---|---|
| F01 | Native header, drag area and Close; remove custom Close | Product, M3.4, M13, P-W3 | 1 | Native Windows/macOS; source/browser insufficient |
| F02 | One Conversations navigation entry | M3.4, M14 | 1 | Rendered toolbar and exact navigation |
| F03 | Notification types as icon/count counters | M3.4, M14 | 1 | Existing canonical New/Needs me data and access names |
| F04 | Remove Home Tasks heading and its space | Product, M3.4/M3.6 | 1 | Rendered Home layout |
| F05 | Align project Back/breadcrumb/heading | M3.8, TW1 | 2 | Narrow/long project header |
| F06 | One shared Antigravity mark, compact icon/count | M3.24, visual-style assets | 3 | Shared asset, strip and runtime surfaces |
| F07 | No standalone compact Partial label | Product, M3.24 | 3 | Lower bounds/unknowns and accessible explanations |
| F08 | Strip starts at sticky position without top gap | M3, M3.1 | 3 | Scroll sequence, not still image |
| F09 | Sigma total first | M3.3, task-counters | 4 | Order, total, accessible name and filter scope |
| F10 | Subtle unselected Held outline | task-counters | 4 | Both themes, selected/hover/focus and Start retained |
| F11 | Current dropdown and previous-section swap | M3, task views | 4 | Distinct choice, same choice, repeated swap, initial state |
| F12 | Quick existing grouping toggle | M3, task views | 4 | Shared setting; preserve section/filter/query |
| F13 | Entire task row opens exact Conversations task | M3.9/M3.10/M3.14/M3.16/M3.17 | 5 | One keyboard/pointer target, controls moved to detail |
| F14 | Same task template; distinct related session template | project-blocks, unified reader | 5 | Shared bytes/code, real model/effort or unavailable |
| F15 | Sticky compact head, flowing metadata, one history | unified reader, TW2 | 5 | Scroll/narrow/long-content behavior and preserved actions |
| F16 | Chronological shared copyable history across attempts | unified reader, D1/D4, TW5 | 5 | Source order, per-block provenance, questions/answers/receipts |
| F17 | Exact event/question deep links and header jump | unified reader | 5 + owner interaction | Target survives paging/reopen; source mismatch refuses |
| F18 | Work sessions join All/In progress | unified reader, Conversations sidebar | 5 | Exact objects retained; no separate navigation mode |
| Q01 | Explain the Antigravity process failure | R13, shared F1/F5/F6, operator D4/D10 | 3 | Responsible diff, requirement, missed check and minimal prevention |
| F19 | Preserve one shared task pin at the top beneath filters; pins survive filtering in Home and Conversations | P-W1, Conversations sidebar | Owner steer 6627872260; text only | Shared persistence/readback, filter/query survival, deduplication and safe unpin |

Initial-state detail for F11: omit Previous until a distinct second section has been selected. No invented previous destination or new persistence requirement. F03 reuses existing New/Needs me semantics rather than inventing notification categories.

F19 is the later pin clarification from owner steer 6627872260, assigned this stable number for tracking; original F01–F18 and Q01 numbers are unchanged. No new screenshot was supplied with that clarification.

## Q01: Antigravity addition

The responsible permanent-strip addition is [015dd31a12d0bc0e31aaf7ef8db17a14100c0fee](https://github.com/taskqorg/taskq/commit/015dd31a12d0bc0e31aaf7ef8db17a14100c0fee), merged by #2152 for task #2150. It added Antigravity to the seeded host/overall runtime maps and expanded the table, but did not add a runtime asset or update the generic `bot` fallback and non-Codex/non-Claude text-label branch. Antigravity therefore became permanently visible through the unknown-runtime fallback. The table CSS hid that fallback label in host rows while the compact overall row retained it.

The commit did amend M3.24 and records an owner approval and count-reproduction before/after. This is evidence of partial spec-first work, not proof that no process was followed. The concrete omission was the shared visual pattern/asset and its acceptance boundary: shared fidelity F1 already requires shared tokens/assets; operator D4/D10 require compact meaningful labels and symbol/count consistency. No Antigravity provider asset was included in that diff. Its changed tests required the third runtime count and preserved uncertainty; the browser checks recorded counts/layout, not recognizable shared identity or absence of a redundant runtime label. Passing those checks did not establish design fidelity.

Smallest prevention: reuse those existing rules, add the shared official mark once, and extend the existing runtime-strip regression to require the supported runtime families to use their shared marks without redundant compact labels. Keep its count, unknown, deduplication and freshness checks. No additional agent-behavior instruction is needed or applied. Native platform and owner visual acceptance remain separate under R12/F5/F6.

## Implementation and verification

The implementation covers F01–F19 together. No session was launched, deleted or replayed for these checks; browser instruments use invented snapshots and read receipts. #2338 remains separate; this change does not claim that repair or change its state. Source/native absence remains unknown (R12).

| Finding | Candidate evidence | Disposition and limit |
|---|---|---|
| F01 | Decorated Menu/Settings/View/Conversations windows, native caption drag tracking and frame-aware sizing; `window-chrome.browser.cjs` | Source/browser pass; native Windows/macOS drag, Close and frame acceptance **not tested** |
| F02–F04 | `owner-ui-review.browser.cjs`, `window-chrome.browser.cjs`, notification component checks | Pass at the browser/component boundary: one entry, New/Needs me counters, no Home heading |
| F05 | Contextual project header at 320/360/800 px in both themes, `window-chrome.browser.cjs` | Browser pass; native font/owner visual acceptance remains separate |
| F06–F07 | `tray-running.test.cjs`, `project-blocks.test.cjs`, `menu-host-strip.browser.cjs` | Pass: shared provider mark, icon/count pattern, lower bounds and unknown explanations retained |
| F08 | Initial three-pixel scroll in `owner-ui-review.browser.cjs`; host/filter stacking and wheel instruments | Browser sequence pass; still images alone do not prove sticky movement |
| F09–F12 | Total order/name, theme outline source, dropdown/current/previous, unchanged choice, grouping and view persistence instruments | Browser/component pass; appearance remains for owner acceptance from the supplied images |
| F13–F14 | Same renderer/HTML for Home/sidebar; eight-state route/action tests and compact-row layout matrix | Pass: one accessible row target; controls moved to detail; session identity stays distinct |
| F15 | `task-navigation.browser.cjs`, dashboard health metadata, wheel/long/narrow reader checks | Browser pass: fixed compact header, flowing metadata and one chronological reader |
| F16 | Accepted board timeline; bounded merged worker pages; source models and unavailable metadata; Copy; overflow and worker instruments | Pass at source/browser boundaries; unavailable native history stays explicit |
| F17 | Immutable question/event paging; notification line-to-event handoff; accepted attempt keys; closed-report generation rejection, anchor return and explicit Latest tests | Pass at source/browser boundaries; missing events retain their target without substituting a current question |
| F18 | Work sessions legacy route resolves to In progress; task/session rows use normal sidebar and common reader | Browser pass; underlying sessions/history retained |
| Q01 | Responsible commit and rule/asset/verification trace above; runtime-strip regression retains counts and uncertainty | Complete; existing instructions reused, no new behavior policy applied |
| F19 | `tray::tests::task_pin` (2 cases), dashboard and Conversations-centre browser instruments | Pass: shared native readback, insertion order, filter/query survival, legacy union, exact unpin and failed-storage preservation; native UI untested |

Original observations and full expected results remain authoritative in the retained issue notes. The specification table above preserves each stable finding number, affected screen and screenshot reference; combined verification rows do not merge or renumber findings.

## Failure before, pass after

- Before the shared-row fix, the regression observed two buttons where one whole-row target was required (`2 !== 1`). After: one row button, no nested board/action/detail targets; the shared output and exact route checks pass.
- Before the provider fix, the shared Antigravity mark was absent. After: the official asset is pinned and used by the runtime strip and task/session surfaces; the original count, deduplication, freshness and unknown checks still pass.
- Exact historical question outside page one: the request named its immutable ID, but the reader rejected the returned source cursor; the new reproduction timed out. After: question/event identity validates the page independently of its numeric cursor; the same instrument passes.
- Pin persistence: the first-save test read back `null` instead of the new dashboard because its root schema was absent. After: first-save/reload succeeds; invalid task and failed-storage cases preserve prior pins.
- A focused row tooltip exceeded a 320 px viewport; a View editor remained on Completed after choosing All. After: right-aligned bounded tooltip and same-selection editor reset pass the original narrow/view checks.
- Historical native pages formerly replaced the previous worker document. After: loading the earlier worker retains newer blocks, and equal source offsets have distinct attempt-qualified keys. Replacement generations remain refused until explicit Latest.
- Canonical approval readback initially missed all named UI approvals: accepted held-intake notes had advanced the board revision, but the gate retained the original Hold revision and ignored the later Start. The new fixture fails before and passes after updating note revision tracking without granting Held notes rule scope. The actual gate now recognizes the approved UI amendment ask.
- Notification entry handoff discarded the native event line, and the reader ignored legacy event context. The new older-event regression timed out before the fix. After: Menu preserves the line, the exact board/root/task maps it to its event block, and unavailable events retain their original target without substituting another question. The owner-review and Conversations-centre instruments pass.
- Shared pin readback now unions legacy task pins without mutating preferences on read. The focused native regression verifies deduplication, exact unpin, unknown-identity preservation and partial storage failure; browser checks verify placement beneath filters and persistence across query/filter/reopen.
- The first canonical Rust run passed 993 tests but failed two old fixtures: closed history assumed one row rather than one question, and the status golden omitted the accepted-history/worker/brief additions. After: the former still requires exactly one original question, source text, evidence URL and immutable conversation identity, selected explicitly from the expanded history. The golden adds only the new fields; a recursive comparison verifies every prior value unchanged. The held-question regression and all three focused status JSON/schema checks pass. The snapshot contract and published schema describe the added read-only fields.
- The next canonical run passed 995 Rust unit tests, 21 integration tests and offline selftest, then stopped on the display-model check's old age markup. After: its exact-age comparison includes the shared row's elapsed-time title, and its total check uses approved Sigma. Count/order parity, source-failure causes and loaded totals remain required; all 38 Node component checks pass. This fixture correction changes no product behavior or protection.
- A later browser phase reproduced Back returning from In progress to All after internal task switches. Those switches bypassed the shell navigation stack. After: switching to a different sidebar task captures the existing scope/selection in that stack without creating an external linked-source scope. The unchanged eight-state navigation/Back regression, Conversations preservation suite and owner-review instrument pass; drafts, exact identities and no-send protections remain.

## Canonical verification

Owner answer 6629638004 approves the R6/R12 test scope, and the canonical rule gate accepts it without bypass. Technical metadata flows in Conversations under F15, and the unknown total uses Sigma under F09; health/unknown protections remain covered. The first full Rust run exposed the fixture failures described above. The canonical rerun uses the same immutable base, without another rebase; its exact candidate SHA and outcome are recorded on #2343. Native acceptance and merge-group CI remain separate (R12).

## Reviewer correction: F16 and F19

Reviewer note 6631007056 returned concrete acceptance gaps against the previous candidate `0b6b133b3cb133d79ea77c547365669472660a9a`. This correction implements the existing F16 and P-W1/CS5 requirements; it changes no owner decision or agent policy.

- Long tool results replaced the text-pagination controls with a single truncated `<pre>`. The added case in `conversations-overflow.test.cjs` failed with `tool results expose Next text`; after the fix it traverses to the last page and back while retaining the full original result for Copy. Tool output remains literal text.
- Answered questions omitted the source role. The history regression failed on missing `answered.source.role`; it now retains the original reviewer role, attempt and session through an accepted reviewer ask/answer sequence. The fixture uses a valid result SHA so the real review transition is exercised. Original worker questions, immutable question IDs, answer text and current-question deduplication remain protected.
- The extended `closed-pins.browser.cjs` was run against a workspace bundle rebuilt from the exact previous candidate: it failed with `0 !== 3` on the independent missing-pin reads. The corrected browser case passes for exact GitHub/GitLab references, first explicit open, 24-hour expiry, unknown-state retention, and manual Unpin. Home and Conversations now use one shared pin reader/lifecycle. Unknown sources never acquire another task's identity or expire as closed.
- Unavailable saved pins previously failed native Unpin with `Pin project missing`. The existing native readback regression now removes the exact saved reference and legacy key without a project lookup, preserving unrelated pins, archive preferences and partial-storage error protection. An unavailable row remains one navigation target; Retry and Unpin live in its detail header/reader.
- Finished headers derive Pin/Unpin from the saved reference instead of always showing Unpin. Source-backed Finished tasks can be pinned. A missing closure time persists the first verified observation using the host clock; refresh cannot renew it, restart retains it, and a verified live task clears the old observation without losing its pin. Observation metadata cannot create an unsourced pin. Existing storage/readback failures still preserve the prior state.

The canonical gate is rerun because this correction changes shared source code. It uses the same immutable base `8e8706fdc93ee2285d69c8b314a43dae4de0d8a2`; no second rebase is performed. Its exact candidate and outcome are recorded on #2343. F01 native Windows/macOS header, drag, Close and frame sizing, OS clipboard delivery and owner visual acceptance remain unverified; source/browser checks do not close those gaps (R12). No live installation or host policy changes are part of these fixes.

The first correction gate stopped at compilation (`cannot find function now in the crate root`, 58.187 s). The persistence path now calls the tray's existing local clock. The same focused `cargo +stable test --locked --bin taskq task_pin_ -- --nocapture` passes all three cases, including exact unavailable Unpin, first-observation persistence/reset, source-backed Finished pinning and storage failure preservation. The next full gate includes the same checks; no failed run is reported as passing.

## Checks and artifacts

Host: Windows; Node 24.5.0, Python 3.12.10, Cargo 1.99.0; Playwright 1.64.1. Chromium/WebKit instruments are isolated in the OS temporary browser cache. These are production web bundles with invented source data, not installed/native acceptance.

Focused checks: `npm.cmd run build`; project-blocks, task-workspace, tray-running, tray-answer, worker-steps and Conversations cache/overflow Node checks; Rust source-model, immutable history paging/timeline and pin readback checks. Browser checks: owner UI review, dashboard, task navigation/list, Home header, host strip, window chrome, closed pins, question paging, worker panel and Conversations centre (Chromium/WebKit source return). The canonical post-rebase `python scripts/check.py` outcome and candidate SHA are recorded on #2343; a local gate does not qualify merge-group CI or native OS behavior (R12).

Visual captures, generated after the sole rebase, are `home.png`, `home-light.png`, `project.png` and `history.png`. They show invented data; their fixture and interaction sequence are in `tests/owner-ui-review.browser.cjs`. Native Windows/macOS header interactions and OS clipboard delivery remain unverified. No service, host policy, runtime preference or permissions changed.
