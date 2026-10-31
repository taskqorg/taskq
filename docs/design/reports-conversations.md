# Reports in Conversations: design review

Put questions, reports and finished work in one Conversations window. Keep the grouped sidebar visible while the operator reads and decides. The [owner answer](https://github.com/taskqorg/taskq/issues/1977#issuecomment-6535685297) approved the Product design and six-screen package. Build verification and native qualification remain distinct (A5/A6, R12).

Requirements: [taskq.md Product](../../taskq.md#product), [RC1–RC7](reports.md#conversations-design-checkpoint-1977), [decision cards](answer-card.md#result-decision-candidate-1977), [Conversations placement](tray-workspace.md#reports-and-finished-work-candidate-1977). Shared [design workflow](https://github.com/taskqorg/specq/blob/main/design/workflow.md) and F1–F6 govern approval and qualification. R1, R6 and §2 lifecycle protections are retained; this proposes no queue transition change.

## Baseline and five operator paths

Baseline source: `554552daf48d5b413918d54a8a6d0f88efba4020`, including merged #1930. `src/workspace.js` places the separate `reports.section` before Conversations; `src/task-workspace.css` hides `.conversations` when that section has a `.report-card`. `src/reports.js` implements All results, sidebar open, reactions, reason field and create submit. Source inspection establishes these paths; this is not a timed study or a native measurement.

Every comparison starts with an unrelated report already open in Conversations, populated expanded project groups and no active search. The target is another visible item. A switch means leaving the separate Results reader for the Conversations list; changing the selected thread inside the proposed pane is not a section switch. Clicks include opening the target, disclosure/action and submit; typing and scrolling are excluded and stated below. No extra Results toggle is charged when Back already reveals the list. These are fixed scenario counts, not claims that every possible entry route improves.

| Job and endpoint | Current path | Current switches / clicks | Proposed path | Proposed switches / clicks |
|---|---|---:|---|---:|
| Answer another task question; receipt shown | All results → question row → choice | 1 / 3 | Question row → same choice | 0 / 2 |
| Accept another report; acknowledgement shown | Unavailable: report choices are Make tasks, Later and Drop | Unavailable | Report row → 1 Accept | 0 / 2 |
| Reopen or follow up another finished task; held-draft receipt shown | All results → result row → Reopen/Follow-up → type reason → Create held draft | 1 / 4 | Result row → Other actions → Reopen/Follow up → type reason → Send | 0 / 4 |
| Read another finished task's Changed/Checks/Open | All results → result row | 1 / 2 | Result row | 0 / 1 |
| Switch project to a question; content shown | All results → other project's row | 1 / 2 | Other project's row | 0 / 1 |

Of the four currently supported paths, none is worse: three remove one click, correction ties, and all four remove a section switch in this starting condition. Report Accept is a new two-click path with no equivalent current action. Both correction paths require the same one-line reason. The proposal trades the current direct correction button for Other actions, paying one click back to keep the primary card small and preserve the four-choice limit. A collapsed target project adds one disclosure click to both supported paths. At narrow width, List replaces the return click and Other actions starts expanded to avoid an extra correction click; native narrow paths still need qualification. Scroll distance is not measured; long reports and 20 px type remain layout/qualification cases.

The proposed report Accept acknowledges only; Make tasks remains an explicit separate decision. Ordinary finished results already have an Accept acknowledgement, but substituting that path would not measure the requested report job. The owner approved the acknowledgement meaning and this unavailable-baseline qualification for A3.

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
| A1 | Root Product amended first; Conversations/RC1–RC7 define destination, grouping, filters and independent read/decision state | Approved |
| A2 | Real shared renderer in prototype; all five reactions and held-proposal decision specified with choice/confirmation/Next rules | Approved; typed shared-card adapter implemented, focused checks pass |
| A3 | Four supported source-derived wide paths: 0 worse, 3 fewer clicks; report Accept is currently unavailable | Approved baseline qualification; native/narrow timing not tested |
| A4 | Six distinct reference screens and editable fixture; 72 layout cases | Approved six-screen mockup package |
| A5 | Production source integrates results and PM outcomes in Conversations; wide Windows candidate capture passes | Build behavior verified; live installed activation unverified ([existing-host ledger](evidence/1977/windows-current/README.md)) |
| A6 | Decision/controller, receipt and grouped-sidebar instruments pass; both themes captured in Windows WebView2 | Windows structural comparison partial at owner 140% text; Mac connection refused; native-input cells Not tested |

Implementation evidence: `npm.cmd run build`, `node tests/answer-card.test.cjs`, `node tests/report-decisions.test.cjs`, `node tests/reports.test.cjs`, `node tests/reports-cache.test.cjs` and `node tests/reports-bulk.test.cjs` pass. The bundled browser instrument covers 16 surface/theme/width/font cells, including local result reading, held successors, source-bound retries, PM-linked membership, Next, exact-SHA links and stable history cursors. Existing grouped-sidebar preservation remains separately checked by `tests/conversations-sidebar.browser.cjs`.

Failure before: the new Rust report-decision case rejected report Accept with `reports use Make tasks, Later or Drop`. The implementation adds SHA-bound report acceptance while retaining first-winner reservations and held-only creation; the first implementation canonical run passed this case. Browser diagnosis also reproduced a newer collected decision being replaced by an older successor receipt; the focused regression now preserves revision 20 acceptance over revision 10 while retaining the held successor. At narrow width a rotated disclosure arrow overflowed by 4 px; its bounded column now passes both font sizes.

The earlier canonical design run stopped at `rule gate: the task does not name Product (changed)`. No guard was bypassed; the owner subsequently approved Product explicitly. The first implementation run then passed 743 Rust cases, including report acceptance, with 39 ignored; the retained contract-size guard failed because the Product wording exceeded 60 KiB by 162 bytes. Compressing only that Product pointer retains its linked requirements and brings the contract to 300 lines and 61,429 UTF-8 bytes. No assertion changed. The final frozen implementation must pass `python3 scripts/check.py --base 554552daf48d5b413918d54a8a6d0f88efba4020`; its exact result is recorded on the task and PR.

Before A6, use one exact build candidate and identical fixtures on both native engines; record source/build identity, scale, theme, text size and usable area. Native evidence and installed-app acceptance remain unverified until observed. Section 7.8 keeps live release acceptance in the owner batch; no service restart or live installation is part of this build verification. Publication still requires green merge-result CI (R12).

## Windows instrument capture

The isolated WebView2 debug candidate produced [report](evidence/1977/windows/popover-dark-reports-conversations-16.png) and [finished task](evidence/1977/windows/popover-dark-results-conversations-16.png) frames in both themes. [Capture metadata](evidence/1977/windows/capture.json) pins the executable, source and fixture. The report and Changed/Checks/Open content render inside Conversations using its narrow reading layout. These 801 by 601 pixel captures do not qualify the approved wide sidebar geometry; display scale and usable work area were not measured. The decision controls below the reading fold and native interaction remain unqualified.

Launching the bare Cargo debug executable returned loader status `0xC0000135`; the same binary captured successfully through `cargo +stable run --locked`, which supplies its build dependency environment. This is an instrument, not installed-layout or live service proof. macOS capture is not available on this Windows attempt. A5 installed-app acceptance and A6 native reference comparison remain open (R12); continue this same task's qualification stage on the required host after parking the current attempt (R2).

The post-reset canonical run passed the Rust suite and isolated offline selftest, then detected that `src/pm-composer.js` was missing from the published task-workspace input list after result reasons began using it. Refreshing only `docs/design/task-workspace.sources.json` with the existing `--write` command retains the unchanged bundled-input, released-byte and hash assertions; its focused check passes. The canonical gate is rerun after this manifest correction, with results recorded on the task.

## Existing-host continuation

The [owner continuation](https://github.com/taskqorg/taskq/issues/1977#issuecomment-6546915208) removes the disposable Mac prerequisite and requests exact Not tested gaps. The [six-screen native review and acceptance ledger](evidence/1977/windows-current/README.md) records the installed Windows baseline, the wide relocated candidate, measured display/text settings and failed access to the existing Mac. No live installation or native-input acceptance is claimed. The new capture-size change affects only the result snapshot routes; ordinary windows keep their existing size.

Source `aa2cea419fd7a7bcb0e962889983e8fcedd489ae` passed the canonical gate in 299.250 s, including 744 Rust tests, 19 integration tests, offline selftest and the 16 report browser cells. The grouped-sidebar browser instrument also passed. A1-A4 remain approved. A5 activation and incomplete A6 native cells are reported openly for publication review under R12 and §7.8; they do not wait on another disposable-context task.
