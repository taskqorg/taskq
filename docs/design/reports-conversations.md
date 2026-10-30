# Reports in Conversations: design review

Put questions, reports and finished work in one Conversations window. Keep the grouped sidebar visible while the operator reads and decides. This is the A1–A4 design checkpoint for #1977; the merged destination is requested, while the six-screen geometry and detailed interactions await owner approval. A5 build and A6 native qualification are not complete.

Requirements: [taskq.md Product](../../taskq.md#product), [RC1–RC7](reports.md#conversations-design-checkpoint-1977), [decision cards](answer-card.md#result-decision-candidate-1977), [Conversations placement](tray-workspace.md#reports-and-finished-work-candidate-1977). Shared [design workflow](https://github.com/taskqorg/specq/blob/main/design/workflow.md) and F1–F6 govern approval and qualification. R1, R6 and §2 lifecycle protections are retained; this proposes no queue transition change.

## Baseline and five operator paths

Baseline source: `554552daf48d5b413918d54a8a6d0f88efba4020`, including merged #1930. `src/workspace.js` places the separate `reports.section` before Conversations; `src/task-workspace.css` hides `.conversations` when that section has a `.report-card`. `src/reports.js` implements All results, sidebar open, reactions, reason field and create submit. Source inspection establishes these paths; this is not a timed study or a native measurement.

Every comparison starts with an unrelated report already open in Conversations, populated expanded project groups and no active search. The target is another visible item. A switch means leaving the separate Results reader for the Conversations list; changing the selected thread inside the proposed pane is not a section switch. Clicks include opening the target, disclosure/action and submit; typing and scrolling are excluded and stated below. No extra Results toggle is charged when Back already reveals the list. These are fixed scenario counts, not claims that every possible entry route improves.

| Job and endpoint | Current path | Current switches / clicks | Proposed path | Proposed switches / clicks |
|---|---|---:|---|---:|
| Answer another task question; receipt shown | All results → question row → choice | 1 / 3 | Question row → same choice | 0 / 2 |
| Accept another finished result; acknowledgement shown | All results → result row → Accept | 1 / 3 | Result row → 1 Accept | 0 / 2 |
| Reopen or follow up another finished task; held-draft receipt shown | All results → result row → Reopen/Follow-up → type reason → Create held draft | 1 / 4 | Result row → Other actions → Reopen/Follow up → type reason → Send | 0 / 4 |
| Read another finished task's Changed/Checks/Open | All results → result row | 1 / 2 | Result row | 0 / 1 |
| Switch project to a question; content shown | All results → other project's row | 1 / 2 | Other project's row | 0 / 1 |

None is worse; four paths remove one click and all five remove a section switch in this starting condition. Both correction paths require the same one-line reason. The proposal trades the current direct correction button for Other actions, paying one click back to keep the primary card small and preserve the four-choice limit. A collapsed target project adds one disclosure click to both paths. At narrow width, List adds one click to the proposal; correction is then one click longer than this wide baseline. Native narrow paths must be measured separately, not included in the “none worse” wide comparison. Scroll distance is not measured; long reports and 20 px type remain layout/qualification cases.

Report acceptance is new: the current report-only card has Make tasks, Later and Drop, with no Accept action. The comparable existing Accept above is the ordinary finished-result acknowledgement. The proposed report Accept acknowledges only; Make tasks remains an explicit separate decision. No invented current report-accept path is counted.

## Preservation and build ledger

| Existing capability | Proposed destination / change | Build boundary and check |
|---|---|---|
| Open question, source IDs, recommendation, literal answer, attachments and receipts | Same task history/card/composer in Conversations | Keep shared card and workspace answer guards; no consequential close/publication authority |
| Owner dialogue, linked task journey, drafts, Watch and native history | Same conversation and Details | Keep originating PM outcome; deduplicate standalone task identity; verify draft/anchor/focus on selection and refresh |
| Pinned / project / Other, recency, collapse, inline actions | Same sidebar plus intersecting filters | Extend `conversations.js` membership/search fixtures; retain source timestamps and unknown provenance |
| Ordinary result Changed/Checks/Open, review/PR/evidence | Finished thread reading pane | Reuse `reports.js` field parsing and immutable cached content; no UI board fetch |
| Report Question/Recommendation/proposals/full text | Report thread reading pane | Keep untrusted text rendering; immutable SHA content and readable proposal disclosures |
| Make tasks, claim identity, partial creation, Later, Drop | Shared cards and typed result adapter | Preserve reservation, first winner and stable per-item claims in `reports.rs`; add report Accept with readback |
| Result read/accept and report markers | Same board truth; display read separately from decided | Test old-SHA markers, Accept vs pending Make tasks, unknown readback and collection overlays |
| Completed history, closed pages, older results | Finished filter, same cursor | Expose decided rows from disposable collection; retain unavailable data with reason and avoid pagination skips |
| Notifications, legacy Results routes, CLI aliases | Notifications deep-link to exact Conversations thread; `results`/`reports` unchanged | Remove independent decision controls/reader; verify direct link, Back and same source identity |
| Archive/Restore, permanent deletion of owned local request records | Existing local Archive and request-only Delete | Results have no board-delete control; no new execution, task or source-file deletion authority |

Selection is board/project/task identity, with result SHA inside its history. Grouping uses recorded registered project identity, never title matching. One independently opened task row spans question, work and completion. A PM thread keeps its linked outcome without automatically adding a second standalone result row. An independently opened task may remain alongside its PM conversation; those are two different conversation contexts, not two result rows.

All is default. Needs reply and Needs decision reveal matching items across project groups; Finished keeps decided work. Search intersects the filter and temporarily reveals collapsed groups. Read/Later does not mean decided; Accepted/Dropped/Drafts created do. A collection failure never becomes “no results.” Local Archive is separate from board decision truth. More and Next preserve the selected filter; no next item offers All, rather than quietly changing project/filter. RC1–RC7 own these rules; this ledger maps them to the existing implementation.

## Six-screen reference

All data is invented. The editable [mockup](exploration/reports-conversations.html) uses the real `src/answer-card.js` renderer and existing answer-card/Conversations styles, with local-only navigation and action simulation. It does not implement the result adapter or board writes. Header window controls, pin/archive, More, native attachments, full dialogue persistence and installation are shown or preserved in the ledger, not qualified by the simulation. The correction form is a design representation of the shared composer, not its production controller.

| Screen | Decision or state |
|---|---|
| [1 · Merged sidebar](evidence/1977/01-question.png) | One list of grouped questions, reports, finished tasks and owner conversations |
| [2 · Report](evidence/1977/02-report.png) | Question, recommendation and shared primary decision card; proposals/full text stay in the same reader |
| [3 · Finished task](evidence/1977/03-finished.png) | Changed/Checks/Open and correction/follow-up question |
| [4 · Empty](evidence/1977/04-empty.png) | No conversations versus no filtered matches; no false board-wide clear state |
| [5 · Many items](evidence/1977/05-many.png) | Scrollable grouped list; loaded counts explicitly incomplete |
| [6 · Unknown delivery](evidence/1977/06-unknown.png) | Cached reading stays available; conflicting actions disabled; Check delivery retains intent |

Reference capture: Chromium headless on Windows, system-ui (Segoe UI), dark theme, 16 CSS px, device scale 1, viewport 1280×1000; the app crop is 1280×760. These are browser mockups, not native Windows screenshots. [Instrument results](evidence/1977/prototype-checks.json) include renderer version, source baseline and the 72 cells (six scenes × three widths × two fonts × two themes). No theme/size-only variants are added to the owner review set.

Run after `npm ci --ignore-scripts --no-audit --no-fund` and `node scripts/build-web.cjs`, with the documented Playwright setup in `tests/browser.cjs`:

```text
node docs/design/exploration/reports-conversations.check.cjs
```

## Checkpoint and remaining evidence

| Acceptance | Observation | Status |
|---|---|---|
| A1 | Root Product amended first; Conversations/RC1–RC7 define destination, grouping, filters and independent read/decision state | Design package ready for review |
| A2 | Real shared renderer in prototype; all five reactions and held-proposal decision specified with choice/confirmation/Next rules | Design package ready for review; adapter not built |
| A3 | Source-derived wide paths above: 0 worse, 4 fewer clicks, 5 fewer section switches | Pass within stated starting condition; native/narrow timing not tested |
| A4 | Six distinct reference screens and editable fixture; 72 layout cases | Pass as mockup evidence; owner geometry approval pending |
| A5 | No production code or installed app changed | Not tested / not built |
| A6 | No approved-reference native Windows/macOS comparison or production decision/grouping tests | Not tested |

Candidate verification: `node scripts/build-web.cjs` passed, and the design instrument passed its interaction checks and 72 geometry cells. An initial canonical run found a trailing blank line in the new instrument; it was removed and `git diff --check 554552daf48d5b413918d54a8a6d0f88efba4020` then passed. The next `python3 scripts/check.py --base 554552daf48d5b413918d54a8a6d0f88efba4020` passed the selector's own checks and stopped at `rule gate: the task does not name Product (changed)`. Full Rust/Node/selftest execution was not reached. No guard was changed or bypassed. The owner checkpoint must name the Product decision as well as approve the design; the draft PR is not a publishable result.

Approve the six-screen package to continue the same task's build, or request a design revision. This checkpoint does not ask to split implementation into another task. Before A6, use one exact build candidate and identical fixtures on both native engines; report source/build hashes, scale, theme, text size and usable area. §7.8 keeps live release acceptance in the owner batch; no service restart or candidate installation is part of this design checkpoint. Review publishes only through green merge-result CI; local checks do not establish that gate (R12).
