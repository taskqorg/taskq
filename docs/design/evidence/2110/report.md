# Conversations root-cause correction

## Checkpoint 1 — before implementation

Candidate baseline: `9e0806a48c8221d5a3b10a1f0033370f26e29542`. Owner request and exact presentation-rule authorization: [#2110](https://github.com/taskqorg/taskq/issues/2110). Corrective packet SHA-256: `42fe25860075c0b945a4d80d8172edcaca814abf003bf7c59c87b1355adec20d`; independent native before: `d70f8a0e74178daaff18074c42202b076a2989b0`, 4.6.59-dev.1793516414. Those observations are failures, not candidate acceptance (R12).

Applied rules: R1/R9 preserve authority; R8/R13 require canonical presentation changes first; §7 requires one isolated branch, selected checks and normal publication. No production input, task answers/admissions, report decisions, installation or service changes are part of these checks.

### Causes and reach

| Failure | Source mechanism | Required reach |
|---|---|---|
| R01 current content buried | workspace inserts news and answered cards before current content; event-level read buttons multiply noise | workspace task/result/linked readers, notification history |
| R02 paragraph return | saved board position is captured only once; binary search assumes nonoverlapping anchor rectangles; draw, observer and browser each correct position | shared reader capture/restore, report paragraphs, transcript prepend/eviction |
| R03 Work mode Back | workspace launch carries operation/root, no originating surface/route | Settings launcher, native handoff and workspace Back |
| R04 empty Completed return | view-select persists Completed before forwarding | dashboard selection/restore and compatibility route |
| R05 hidden scope | collapsed summary has no active criteria | Conversations Completed summary |
| R06 chrome density | search-open survives empty blur; linked context duplicates a group heading | shared sidebar/search/view controls |
| loading pauses | repeated full projections; all owner bodies in snapshots; all matching rows mounted; growing query maps | native snapshot/body reads, shared sidebar window, caches, projection callers |
| competing attention | bell counts event lines; Conversations counts different message records | shared canonical attention identity, menu and centre |
| time width | long relative grammar reserves 14ch on every row | existing relative-time formatter and metadata geometry |

The existing native byte cursors, source generations, 256 KiB reads (up to a 16 MiB single record), 200-item pages, 600-item resident window, selection protection and two-viewport eviction are retained. Wheel handlers are passive; wheel interception is not the cause. Durable drafts/files, pending operations, receipt fences and Codex/Claude adapters are retained.

### Fixed budgets

These bounds are fixed before implementation; failed measurements remain failures. Native performance remains Not tested until installed owner acceptance.

- Sidebar: 64 rows per page, at most 192 resident rows (three pages), two viewport edge overscan. Lightweight source identities/counts are independent of fetched bodies; source index ceiling 16 MiB. Exhaustion reports partial/unknown rather than zero.
- Reader: 600 native records plus one in-flight 200-record page; 32 MiB resident text, bounded by the existing 16 MiB per-source-record ceiling. Selected text and pending/durable edits are protected, with any pinned overflow reported explicitly.
- Inactive body cache: at most 8 sources and 8 MiB; query/cursor caches at most 24 keys. Evicted bodies re-read from their source. No per-row network requests.
- Existing saved anchors: 24 sources / 1.5 KiB within the 4 KiB navigation envelope. Unavailable source/position is explicit; only Latest resumes follow.
- Fixed synthetic fixture: 600/10,000 thread identities; 100,000 native records; five paging/search/return tours; one large Unicode record. p95 settled interaction ≤100 ms and scroll frame ≤32 ms on the recorded warm local headless browser. Initial projection+layout ≤100 ms, independent of total heavy body bytes. Native timing is a separate observation.

### Before measurement

Mac, 8 logical cores; Node 22.18.0; headless Chromium 156.0.8078.4; warm bundle, 800×600, 20px, dark. No native timing/heap profile exists. A fixed invented draft fixture repeats `Invented saved body ` 150 times per thread. Transport counts UTF-8 JSON bytes; parse/projection/layout use performance.now. Heap readings without exposed GC are not retained-memory proof.

| Threads | Transport bytes | Parse ms | Projection/render ms | Forced layout ms | Mounted rows |
|---:|---:|---:|---:|---:|---:|
| 600 | 1,879,802 | 0.8 | 66.9 | 2.3 | 600 |
| 10,000 | 31,348,002 | 11.7 | 1,883.7 | 48.3 | 10,000 |

### Preservation and verification scope

The existing [72-capability ledger](../../exploration/conversations-preservation/features.json) remains the inventory; no removal is approved. M01–M32 keep menu/views/project/task and all twelve Settings destinations. C01–C24 keep owner identity, local state, source paging, native input/delivery, files, history, watch, detach/archive/delete, IME and accessibility. A01–A10 keep question identity/choice order, drafts, files, stale-source refusal and exact receipts. X01–X06 keep consumers, authority and qualification boundaries.

Changes reach existing Preact modules, canonical details, their component/browser fixtures, Rust snapshot/IPC tests and the module inventory. Existing native adapters and lifecycle do not change. Checks reuse the current actual-bundle browser instruments and source fixtures; no parallel renderer or new dependency ships. Unexercised capabilities remain Not tested, never certified from a screenshot.

## Checkpoint 2

Implemented in the existing source modules: one attention entrance, Play-only Held, primary current cards with grouped history, compact times, scope-preserving navigation, native origin Back, lazy owner/result bodies, 64/192-row sidebar windows and bounded query/body caches. Native catalog scanning is asynchronous with one bounded refresh, a two-second interval and explicit unknown coverage after four seconds without a fresh observation. Direct source/revision checks still own writes. Installed acceptance remains separate.

## Checkpoint 3 — exact-source handoff

Frozen implementation: `4ab0782e2aed5430653534a50061a88d7ebd0383`. The A5 implementation and heap measurements are at `33eda27f77847dd98fecf8f52a75abf25a0b4ba8`; the final exact diff only adds two protected-overflow notices and their regression check. The subsequent evidence-only commit does not change that implementation. [Owner correction](https://github.com/taskqorg/taskq/issues/2110#issuecomment-6556477418) explicitly supersedes Claude-specific A8 review: implementation and independent exact-source review continue in Codex, without waiting for quota or starting another task/session. The [independent review](review.md) found no P0/P1 source defect. This is a worker handoff, not complete acceptance: the fixed frame budget fails, the canonical host check is sandbox-blocked, and installed native acceptance remains Not tested (R12/§7).

### Failure before / protection after

- The baseline mounts 10,000 rows and transfers every owner body (31,348,002 bytes). Source snapshots now carry bounded headers; sidebar pages have 64 rows / 192 resident. Body reads bind registered root, task or owner identity, source revision, cursor and generation. The native 10,000-task fixture separately verifies that a heavy full snapshot becomes a header snapshot under 3 MiB while the original bodies remain unchanged. The full lightweight envelope has a 16 MiB ceiling; overflow returns an explicit unknown/partial envelope, never source deletion or a fabricated zero total.
- The initial owner-only correction still shipped current task bodies and eager PM-linked cards. That A5 gap is corrected at the native workspace projection, after canonical state preparation. The selected task body is read off the event thread, its historical prefix is stripped, and the parent applies the existing Pending overlay before thinning history and hydrating the scoped local answer draft. Home, OS delivery and native authority continue using their existing full canonical state. Closed source reads retain exact registered board/task/revision checks.
- Current task bodies use two slots / 2 MiB; owner and result bodies each use two / 2 MiB; closed transcript and recorded-question bodies each use one / 1 MiB. Their combined inactive budget is eight sources / 8 MiB. Active text, durable edits and unacknowledged operations remain protected. Existing 32 MiB native-reader and 16 MiB single-record limits remain; protected native/task/closed/question overflow is disclosed. Query/anchor/text-page maps retain their fixed bounds. Partial index observations retain only bounded inactive metadata and preserve active edits.
- The originating PM displays one 64-header page and reads only its selected task body. Current, closed and report identities remain available; Previous/Next and independent loaded/more status retain scope. Report proposals mount 64 summaries per page; proposal prose is lazy and bounded. Existing Make tasks and exact frozen admission reviews remain unchanged and were not exercised against the board.
- First Claude review found two intermediate data-loss regressions: cached absence deleted a confirmed new draft/operation, and a thin snapshot erased sent text/files. `conversations-cache.test.cjs` failed before and passes after epoch/absence fencing and immutable-body merge. An archived question counted as Needs me outside the current source scope; its notification case failed 1 versus 0 before and passes after, with board state still `ask`.
- A 1.95-million-character Unicode record blocked the unpaged probe. The final 2,430,000 UTF-16-unit / 2,790,000 UTF-8-byte record mounts at most 32 spans / 32,770 units, with exact concatenation and no duplicated seams. Current question, historical question, board brief and report prose reuse bounded text pages; images and answer validation retain their original exact source. The earlier focused receive plus layout observation was 38.4 ms; this does not pass the frame budget.
- Closed native anchors retain immutable result root/task/SHA, page end and generation separately from sidebar identity. The centre fixture failed before correction and passes after body eviction and reread of the exact saved page. An unavailable requested anchor cannot be replaced by another visible paragraph; lost positions issue no automatic tail read. Only Latest resumes follow. Owner windows are released when selection changes; counters include owner and worker windows.
- The connected operator-flow fixture exposed a 112 px board-answer jump because the saved anchor was its oversized question wrapper. Stable inner paragraph/choice/evidence anchors now pass the unchanged ≤2 px return/reflow assertion. A source-generation replacement also retained 30 old items before correction; it now removes the old generation and passes the zero-old-items assertion while preserving draft/caret. Hidden disclosure nodes and duplicate wrapper keys no longer compete for the one native scroll anchor.
- Recorded questions read 64 whole records / 256 KiB, with independent totals and revision/cursor fences. The 100,000-question source fixture includes large Unicode text, backward seams and stale/cross-project refusal. During service polling lag, canonical Pending retains the accepted question/answer as a separate confirmed tail while older history pages use the immutable prefix revision and count. Existing accepted-question/answer guards remain unchanged.
- Fixture corrections preserve protection: native adapter traversal uses the first visible record's exact byte cursor after ignored records; every record, backward/forward equality and saved-record inclusion remain asserted. State-flow fixtures advance real source revisions on transitions. The centre waits for its existing debounced navigation receipt, not a guessed 150 ms delay. Route-failure coverage uses the existing choose-labels action and retains Home/Back/Close/error fences. No decided guard was removed.

The final read-only review also found two reachable P2 omissions: one whole closed transcript record or recorded question can exceed its 1 MiB reserve (up to the existing 16 MiB record ceiling) while selected. Both lacked the required explicit pin status. `conversations-overflow.test.cjs` fails both cases against `33eda27f` and passes at `4ab0782e`; the source stays readable, a small replacement clears the status, and older history bodies still evict. No ceiling, source, fence or eviction condition changed. Owner drafts and accepted report sources already have smaller native source ceilings.

### Check results

Final gate and focused results are recorded below after the frozen-source run. A blocked check is never PASS; source/browser evidence never qualifies native installation (R12).

- Final `python3 scripts/check.py --base 33eda27f77847dd98fecf8f52a75abf25a0b4ba8`: **sandbox-blocked**, not PASS; 193.490 s / 900 s. Rust: 820 passed / 37 ignored. Integration: 22 passed / 1 blocked / 1 ignored. Blocked case: `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`, `sandbox-exec: sandbox_apply: Operation not permitted`. No guard, permission or grant was changed. The final new test has unknown selector impact and broadens the gate to full; earlier frozen-source gate had the same blocker after 203.218 s. There is no retry to bypass that refusal.
- `node tests/conversations-overflow.test.cjs`: two failing-before cases at `33eda27f`, two passing-after cases at `4ab0782e`, including source preservation, older-page eviction and clearing the status after a small replacement.
- `TASKQ_EVIDENCE_1929=<temporary evidence directory> node tests/conversations-flow.browser.cjs`: Pass for the final actual bundle, synthetic operator exchange and both inspected final captures. This sends no real input.
- Separately completed the selected remainder that the blocked integration gate did not reach: offline `cargo +stable run --locked -- selftest` in the canonical isolated temporary installation; all 37 `tests/*.test.cjs` suites; four shell/release fixtures; 13 changed browser instruments plus the separately recorded final flow. Remainder elapsed 302.245 s. All functional assertions pass. Both 18-cell Chromium/WebKit time matrices, source-loss/eviction/Unicode, scoped Held and notification/route guards pass. Timing-budget results remain separate failures.
- Exact-source Codex review at `4ab0782e2aed5430653534a50061a88d7ebd0383`: no unresolved P0/P1/P2; only source proof. Frozen Rust Native Pending, catalog epoch/sent history and stale-cursor protections remain.
- Green exact-source/affected-OS `merge_group` CI: pending. Installed native acceptance: Not tested.

The supplied `specq@45ff029915bc8e0e53b4523217de636183aa6ad9` contract lint passed: 0 errors, 50,981 bytes / 257 lines. The script came from the verified immutable commit. The repository gate's separate `.specq` checkout is unavailable locally and reports Unknown; CI owns that check. No pin or gate was changed.

### Resource and latency observations

Warm source bundle on the same Mac / 8 logical cores / Chromium 156. The original before fixture was 800×600, the metadata and mixed fixtures 1000×700, 20px, dark. The phases and workload differ, and initial host load was not captured; these are observations rather than a controlled native speedup claim. Driver auto-wait is excluded from product interaction latency.

| Fixture | Source payload | Initial projection (+ layout where measured) | Interaction p95 / 100 ms |
|---|---:|---:|---:|
| Original 600 owner bodies | 1,879,802 bytes / 600 mounted rows | 66.9 + 2.3 ms | Not measured |
| Original 10k owner bodies | 31,348,002 bytes / 10k rows | 1,883.7 + 48.3 ms | Not measured |
| Intermediate 600 owner headers | 89,041 bytes / ≤192 rows | 84.0 ms | 76.7 — Pass |
| Intermediate 10k owner headers | 1,498,241 bytes / ≤192 rows | 53.5 ms | 124.3 — Fail |
| Final 600 mixed task identities, four projects | 89,732 bytes; selected bodies read separately | 16.4 ms | 82.3 — Pass |
| Final 10k mixed task identities, four projects | 1,489,286 bytes; selected bodies read separately | 33.3 ms | 86.0 — Pass |

The mixed fixture checks saved answer/caret hydration, stale replies, one selected PM body per 64-row page, large brief paging and 10,000 report proposals. Removing repeated per-row scans with the existing task index reduced its observed 10k projection from 83.7 to 37.9 ms in the targeted before/after run. The table uses the last final-source observation at `4ab0782e`, not the best sample. The preceding A5 run at `33eda27f` measured 27.1/32.8 ms projection and 73.7/77.6 ms interaction; both runs are retained. Linked tasks sharing one PM retain their task identities but coalesce into one canonical owner thread in the sidebar. [Mixed measurements](mixed.json) retain both sets of 25-tour heap samples and counters; the earlier failing owner-only measurement remains in [bounds](bounds.json).

Both provider browser models traverse 10,000 native records without missing/duplicate records, retaining anchor offsets within 2 CSS px. Claude mounts at most 671 items; Codex 657 (600 resident plus one in-flight 200-item page). At `33eda27f`, page receive/render p95 was 20 / 22 ms and wheel-frame p95 was 67 / 67 ms. The last final-source run at `4ab0782e` measured receive/render 24 / 22 ms and wheel-frame 83 / 67 ms. The fixed 32 ms frame budget **fails**. In the same run, a script-free clone of the actual DOM/CSS with the same wheel input measured 66.7 ms, and an empty-page rAF control also measured 66.7 ms for both runtimes. These controls establish a scheduling floor that itself exceeds 32 ms. They do not attribute the entire product delay: the final Claude fixture is another 16.3 ms above its control, whose precise contribution remains unknown. The product budget remains Fail; installed native performance is Not tested. [Paired measurements](frame-controls.json) retain functional and timing statuses separately. Native 100k-record source paging passed for both adapters; it is source proof, not installed process qualification.

### Retained memory

The 25 mixed tours at `33eda27f` keep the selected task-body cache at two / 257,178 bytes and one mounted answer controller. Total engine heap still warms up: 600 identities 8,829,580→9,852,791 bytes; 10k 16,120,636→17,789,858 bytes. Those samples are not a plateau claim.

The independent reviewer then ran a separate 50-tour, four-project / 10k-task probe, opening eight exact task sources per tour with fake IPC audit capped at 64. CDP forced GC and captured snapshots at tours 5/25/50. Final retained application counters were two task bodies / 64,844 bytes, one answer controller and eight protected answer sources / 1,163 bytes. Exactly two long brief strings remained, each only through `Object.brief`, matching that cache. From tours 5→50, object self-size changed 1,156,264→1,156,760 bytes, strings 1,266,837→1,265,529, closures 164,280→164,184; V8 code metadata grew 1,828,120→2,375,620. No growing application-source or callback retainer was reproduced. Browser used heap fluctuated 7.22–7.96 MB after tour 5; final 7.91 MB.

This substantiates bounded application retention for this fixture and attributes the remaining sampled growth chiefly to V8 code metadata. A longer browser-engine heap plateau remains **Not tested**, not inferred from cache caps. The [independent review](review.md) and [heap summary](heap.json) record the method, category totals and limits. No heap snapshot containing injected browser source is committed.

### Native-before failures

| Native before ID | Corrected source evidence | Installed after |
|---|---|---|
| R01 event-first reader | Current question/result first, grouped lazy history; ordinary result has no Accept | Not tested |
| R02 paragraph return | Exact report/text pages, disclosures and board paragraphs; offset ≤2 px across return/reflow | Not tested |
| R03 Work mode Back | Origin-bearing IPC and handler return to existing Settings window | Not tested |
| R04 empty Completed menu | Forward without persisting Completed; owned View retains advanced criteria | Not tested |
| R05 hidden scope | Project/query/period/outcome summary and scoped Previous/Next | Not tested |
| R06 chrome density | Empty blur/Escape collapses search; one Linked/Return context; compact times | Not tested |

### Scenario matrix

Pass means only the named synthetic/source observation. Native interaction, OS delivery and process qualification remain separate.

| Scenario | Status and evidence |
|---|---|
| S01 Held controls | Pass — 24 geometry cells plus touch; 0/1/many, OR scope, frozen revisions, independent targets; no real admission |
| S02 one entrance/count identity | Pass — canonical sets, archived/unknown/partial scope, theme/font cells, read/clear and mark-all races |
| S03 exact common reader/current content | Pass — deep links, questions, ordinary/report/activity; real OS activation Not tested |
| S04 scope/Completed navigation | Pass — shared order, outside-scope Return, visible criteria, retained advanced View controls |
| S05 paragraph return | Pass — report/text/board fixtures, ≤2 px return/reflow; installed scenario Not tested |
| S06 Work mode Back | Pass — origin/return IPC and native source wiring; visible native focus return Not tested |
| S07 full operator exchange | Pass — synthetic PM→worker→question→answer→reviewer→closed result, saved drafts/files/caret, exact unknown retry; no real input |
| S08 large datasets/record | Pass for mixed four-project 600/10k headers, task/PM/report paging and 100k adapter/question source fixtures; installed runtime/performance Not tested |
| S09 repeated tours/retention | Pass for model/cache/DOM caps and draft rehydration; 25 mixed tours plus independent 50-tour retainers. Longer engine-heap plateau Not tested |
| S10 native scrolling/anchors | Pass for browser append/prepend/eviction/reflow/selection/Latest; no prevented wheel. Installed trackpad/IME Not tested |
| S11 stale/offline/failure paths | Pass — exact source, receipts, cursor/generation and missing-position guards; provider mutation beyond existing prefix fence unqualified |
| S12 compact times | Pass — explicit units, exact local hover/focus, stable nodes/anchors; Chromium and WebKit 18 cells each |
| S13 coalesced history/current action | Pass — source events, no-proposal report, refusal/unknown, exact IDs/read/clear, confirmed history tail |
| S14 geometry/accessibility | Pass for browser geometry/names/keyboard paths; screen reader and installed localization/interaction Not tested; all Settings destinations retained |

### Visual review

The actual-bundle captures were inspected at image resolution. Each shows a distinct path with invented data; pinned visual decisions remain. No installed acceptance is claimed.

- [Menu, 360px / 20px](menu.png): one communication entrance; Held count once, independent Play target and per-task Start.
- [Ordinary result, 360px / 20px](result.png): Changed/Checks/Open first; no mandatory Accept; existing narrow Other actions retained.
- [Report paragraph return](paragraph.png): visible passage 30 after source return, with scoped selection.
- [Outside-scope reviewer](linked.png): one Linked/Return row, preserved Decisions scope and reviewer write boundary.
- [Confirmed history during source lag](pending-tail.png): exact confirmed question/answer separately from paged prefix, one read-only label.
- [Connected reviewer stage](flow-review.png): actual final header/body protocol, read-only reviewer state and retained PM draft.
- [Connected final result, 350px](flow-result-narrow.png): result remains primary after source replacement; draft and Latest remain reachable without horizontal overflow.

### Remaining acceptance

Source implementation and the independent Codex review are complete for this handoff. Fixed frame timing remains Fail and longer engine-heap plateau remains Not tested, with the measured causes and controls above. The canonical sandbox refusal is not bypassed. Linux/affected-OS `merge_group` CI must be green before publication; worker checks and PR readiness are not that receipt (§7/R12). Independent owner-chat acceptance after an installed candidate remains separate. No installation/restart, live PM message, task answer, report decision, real Held admission, grant, capacity or service change was performed.
