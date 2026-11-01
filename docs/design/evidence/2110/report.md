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

## Checkpoint 3 — incomplete acceptance

Implementation and focused source checks are ready for review. Acceptance is **incomplete**: A5 still has the mixed task-card/PM-linked path described below, and the final Claude exact-source review hit the configured account's weekly limit; no verdict was produced. The fixed 32 ms headless frame budget fails. Installed macOS/Windows acceptance remains **Not tested**. Canonical gate results and the immutable candidate SHA are recorded in the PR; a successful worker check does not imply merge-result CI (R12/§7).

### Failure before / protection after

- The baseline mounts 10,000 rows and transports every owner body. The source now transports lazy metadata, mounts at most 192 list rows, and fetches the selected body by exact owner/revision. A stale body reply cannot select another thread or replace a newer revision.
- First Claude review found two data-loss regressions in the intermediate implementation: cached absence deleted a new confirmed draft/operation, and a thin snapshot erased sent text/files. `conversations-cache.test.cjs` fails both before and passes both after the epoch/absence and immutable-body merge fixes. These are regression evidence, not first-round approval.
- An archived question still counted toward Needs me while absent from Recent. The added `notification-panel.test.cjs` case fails with 1 instead of 0 before the source-scope fix; afterward it passes and the board remains `ask`.
- A 1.95-million-character Unicode record blocked the unpaged browser probe until it was interrupted. Bounded text pages retain every code point with no seam duplication; the final 2,430,000-character fixture holds 2,790,000 UTF-8 source bytes but mounts at most 32 text spans / 32,770 UTF-16 units. Latest focused receive plus forced layout: 38.4 ms. No source reread is caused by text paging; the existing EOF poll remains.
- Closed native anchors use the immutable result reader identity (root/task/SHA), distinct from the logical sidebar task identity, and retain the page end cursor/generation. The centre fixture fails before that correction, then passes after reading two other sources, evicting the old body and automatically rereading its exact original page. Loading cannot replace its missing anchor with an unrelated visible paragraph. Inactive owner transcript windows are released on selection change; diagnostics count both owner and worker buffers rather than hiding one.
- Lost saved cursors now show an unavailable position and issue no automatic tail read. Only explicit Latest opens the current source. Late/mismatched generation/cursor replies are discarded before changing resident boundaries.
- Historical questions read 64 whole records / 256 KiB at a time, with registered-project/board/task/revision/cursor checks and independent totals. The 100,000-question Rust fixture includes one large Unicode record, verifies backward page seams, refuses stale/cross-project reads and compares source bytes unchanged.

The post-gate timestamp fixture exposed a saved-anchor race: before the native range arrived, a visible linked task replaced the requested native anchor. The layout owner now retains an unavailable anchor until its node exists; the receipt validates the immutable key captured when the read started. Worker missing-position/generation cases also require explicit Latest. The timestamp fixture now returns real cursor ranges on a reread instead of treating every `after` as EOF; its node/focus/time/offset assertions remain.

Accepted answers also require a separate history tail during service polling lag. A connected Native Pending regression preserves the existing question/answer guard unchanged, keeps the immutable source prefix revision and its count, and transports only the new confirmed tail. The browser shows that confirmed question immediately while reading the older prefix with its own revision. It retains the actual question/answer and receipts while collapsing repeated read-only boilerplate into one history label. No task authority or native pending-operation policy changes.

The 100,000-record Codex/Claude adapter test exposed a fixture error: it used the page start before an ignored-record prefix as the saved message cursor. It now uses the first visible record's exact `at`. Exact forward/backward equality, every record once, ignored-page progress, uniqueness and saved-record inclusion remain guarded. The route-failure fixture now reaches invalid labels through the local choose-labels action; `focus:null` already renders unknown. Its Home/Back/Close/error-fence assertions remain. No decided protection was removed.

### Check results

- `python3 scripts/check.py`: **sandbox-blocked**, not PASS. After 421.401 s, Rust unit results were 817 passed / 37 ignored; integration was 22 passed / 1 blocked / 1 ignored. The blocked case is `telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`. Its guard and permissions were not changed; no full retry was made for this refusal.
- After the post-gate UI/history corrections, `cargo test --locked --bin taskq tray::tests` passed 56 / ignored 1 using the canonical isolated caller/install environment. The unchanged accepted-question/answer guard passes. The final centre instrument waits for the existing debounced navigation receipt instead of assuming it arrives within 150 ms; the exact saved-page assertion is retained. Centre/history checks and both 18-cell browser time matrices pass after the final UI corrections. The direct worker-environment attempt correctly refused a capacity fixture; the isolated run does not grant the real worker capacity authority.
- Offline `cargo +stable run --locked -- selftest`, 36 Node component suites and the changed browser functional instruments passed separately. The shell/readme/release fixtures also passed. These runs do not turn the blocked canonical gate or failing timing budgets into PASS.
- The supplied `specq@45ff029915bc8e0e53b4523217de636183aa6ad9` contract lint passed: 0 errors, 50,981 bytes / 257 lines. Its script was read from the verified immutable Git commit. The repository gate reports its separate `.specq` checkout as Unknown locally; CI owns that check.
- Final exact-SHA CI / `merge_group` is pending. A draft PR and passing focused source checks are not publication or installed qualification (R12/§7).

### Resource and timing observations

Same Mac / 8 logical cores / Chromium 156, warm source bundle. The final metadata fixture is 1000×700, 20px, dark; before was 800×600. These are explicitly different transport phases: old full bodies versus the new metadata protocol. Driver auto-wait is excluded from product timing. Host load was not captured for the first measurement; the final run observed load averages 23.38 / 26.13 / 21.76, so it is not a controlled idle-host comparison.

| Threads | Before body bytes / mounted rows | After metadata bytes / resident rows | After initial projection + layout | Interaction p95 / 100 ms budget |
|---:|---:|---:|---:|---|
| 600 | 1,879,802 / 600 | 89,041 / 192 | 84.0 ms | 76.7 ms — Pass |
| 10,000 | 31,348,002 / 10,000 | 1,498,241 / 192 | 53.5 ms | 124.3 ms — **Fail** |

Five tours through 30 sources plus search/return keep query maps at 24 keys and selected-body cache at two sources / 232 bytes in this fixture. The implemented inactive-reader caches use a conservative four-way partition: owner/result/closed/question: two inactive sources / 2 MiB each, combined at most eight / 8 MiB. Durable edits, active selection and unacknowledged mutations remain protected. Existing owner-preference metadata retains its 8 MiB source guard. Native owner-catalog construction runs off the UI thread; index truncation/staleness is explicitly partial/unknown. These measurements do not qualify every project/task payload: current task-card bodies still arrive in project snapshots, and originating-PM linked task/result cards are not yet fully paged. That mixed-family path remains an explicit **A5 gap**; the owner-only list fixture must not be used to certify its bytes or DOM/model bounds.

Retained browser heap after forced GC (bytes, tours 1–5):

- 600: 4,112,312; 4,196,032; 4,335,972; 4,430,632; 4,581,948.
- 10,000: 5,919,556; 5,918,484; 6,046,760; 6,236,832; 6,382,716.

The model/cache counters plateau within their caps; total engine heap still grows modestly during these five tours. A longer retained-heap plateau is **Not tested**, not inferred from bounded DOM. The fixture's IPC audit log is capped at 64 records with a separate sticky forbidden-mutation flag, so audit retention does not manufacture unbounded application memory.

Both provider browser models traverse 10,000 native records with no missing/duplicate records and preserve anchors within 2 CSS px. Claude mounts at most 671 items; Codex 657 (600 resident plus one 200-item page). Final page receive/render p95: 66 / 41 ms; wheel-frame p95: 67 / 67 ms. The 32 ms frame budget **fails**. A separate empty Chromium rAF probe measured p95 66.7 ms; this limits attribution but does not change or pass the fixed budget. Native 100,000-record source paging passed for both adapters (85.98 s); this is source proof, not installed process qualification.

### Native-before failures

| Native before ID | Corrected source evidence | Installed after |
|---|---|---|
| R01 event-first reader | Current question/result precedes grouped lazy history; ordinary result has no Accept | Not tested |
| R02 lost paragraph return | Report #2022/#2021 fixture, text-page return, disclosure restore and paragraph offset ≤2 px across snapshot/font/width changes | Not tested |
| R03 Work mode Back | Origin-bearing Settings IPC, preserved preceding Conversations context; native handler returns to existing Settings window | Not tested |
| R04 empty Completed menu | Forward without persisting Completed; owned View retains period/project/outcome editor | Not tested |
| R05 hidden scope | Collapsed Completed summary exposes project/query/period/outcome; scoped Previous/Next stays inside that set | Not tested |
| R06 chrome density | Empty blur/Escape collapses search; linked source uses one context/Return row; shared compact times | Not tested |

### Scenario matrix

Pass below means the named synthetic/source observation only. OS delivery, native interaction and end-to-end process qualification are separate.

| Scenario | Status and evidence |
|---|---|
| S01 Held controls | Pass — 24 geometry cells plus touch; 0/1/many, OR scope, frozen revisions, independent targets, no real admission |
| S02 one entrance/count identity | Pass — canonical sets, archived scope, unknown/partial, six Chromium theme/font cells, mark-all race/read/clear protections |
| S03 exact common reader/current content | Pass for source deep links, questions, ordinary/report/activity fixtures; real OS activation Not tested |
| S04 scope/Completed navigation | Pass — shared order, outside-scope Return, visible criteria and retained advanced View controls |
| S05 paragraph return | Pass for attributable #2022/#2021 fixture, ≤2 px after reflow; exact installed native scenario Not tested |
| S06 Work mode Back | Pass for browser origin/return IPC and source wiring; native window/focus return Not tested |
| S07 full operator exchange | Not tested as one complete PM→worker→question→reviewer exchange. Focused draft/files/caret/focus/unknown-receipt and provider adapter guards pass |
| S08 large datasets/record | Partial — owner-only 600/10k client bounds, 100k adapter/question source fixtures and whole Unicode paging pass. Mixed-family/multi-project task-card and PM-linked bounds remain an A5 gap; native runtime performance Not tested |
| S09 repeated tours/retention | Pass for model/cache/DOM bounds and draft rehydration; five heap readings recorded; longer engine-heap plateau Not tested |
| S10 native scrolling/anchors | Pass for browser prepend/append/eviction/font/width/selection/Latest and no prevented wheel; installed trackpad/IME Not tested |
| S11 stale/offline/failure paths | Pass for retained exact-source/receipt/generation/cursor and missing-position fixtures; provider in-place mutation beyond the existing prefix fence remains unqualified |
| S12 compact times | Pass — component validation, explicit units/future/zone/source; Chromium and WebKit each cover 18 width/font/theme cells with unchanged nodes/focus/anchors |
| S13 coalesced history/current action | Pass — retained source events, no-proposal report, refusal/unknown, no stray zero, source IDs/read/clear retained |
| S14 geometry/accessibility | Pass for affected browser geometry/names/keyboard paths; real screen reader and installed localized/native interaction Not tested. No Settings destination was removed |

### Visual review

Five actual-bundle browser captures were inspected at image resolution. They show distinct paths rather than theme/size duplicates; all data is invented. The pinned palette, typography and bundled glyphs remain. No installed/native acceptance is claimed.

- [Menu, 360px / 20px](menu.png): one communication entrance; Held count once with separate Play target; existing task Start remains.
- [Ordinary result, 360px / 20px](result.png): Changed/Checks/Open precede history; no mandatory Accept. Existing narrow Other actions behavior remains.
- [Report paragraph return](paragraph.png): visible source passage 30 after the #2022/#2021 return, with scoped sidebar selection.
- [Outside-scope reviewer](linked.png): one Linked/Return row, preserved Decisions scope, explicit unavailable native destinations and reviewer write boundary.

- [Confirmed history while the source lags](pending-tail.png): exact confirmed question and answer stay visible separately from the paged source prefix; one read-only label, no duplicated inactive answer forms. Empty history disclosures are omitted.

Machine-readable five-tour measurements: [bounds](bounds.json). Functional assertions and timing-budget statuses are separate.

### Remaining acceptance

A5 remains incomplete for the mixed task-card/PM-linked path; it must be completed in this task. The remaining source boundary is `tray::state` after the canonical Pending overlay, plus `workspace.routes.conversations`/`conversations.linked`: replacing only the visible `.map` would leave the eager payload and model intact. Home previews, current question authority, accepted-receipt history tails and PM draft/answer context need the same revision-fenced read contract; they are not certified by the owner-only bound fixture. A8 requires a complete Claude review of the final immutable source. The second attempt produced only a weekly-quota error; see [review](review.md). Fixed performance failures and longer heap-plateau coverage remain open. Independent owner-chat installed acceptance is still required by the task and must not be represented as worker-installed proof. No installation/restart, real PM input, task answer, report decision or Held admission was performed.
