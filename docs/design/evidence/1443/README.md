# Tray and workspace v2 review — #1443

Proposal for owner review under R13 and § 7.5. No UI implementation, rule amendment,
task filing, installation or production effect is included. This is **before**
evidence; no fix or after-pass is claimed.

Review source: `8bf66914f03560c8c3f979523dce7d6781fe5420`, the merged #1442 profile.
During publication preparation, main advanced to
`8669a08ec2435e830301bf7307fb37f477c87e6d`: #1449 implements the #1438
counter-toggle decision. **Read the [current delta](current/README.md) first.**
It overrides historical filter findings and the five briefs below: R6 and its
linked counter contract win over the now-stale v2 checkbox/zero clauses (R13).
The original measurements remain source-labelled text; the updated instrument
checks every current route without writing screenshots. Nothing in this proposal restores the superseded checkbox UI.
Rules: [TaskQ v2](../../ui-rules-v2.md) and its exact
[38 shared checks](https://github.com/taskqorg/specq/blob/d54981e576a80c3854b455c5eb1a441a676f4497/design/app-ui.md).
The shared revision is a review input, not proof of upstream merge or deployment.

## Evidence boundary

The owner updated A1/A2 to cap the review at ten screenshots, one per distinct gap.
[Eight gap screenshots](screenshots.md) are retained; the other 394 PNG files were
removed. Passing matrix cells commit no screenshot. No plan option has been chosen.

The [instrument](capture.cjs) extends #1348's real-bundle Playwright check using
`tests/browser.cjs`, invented answer/resource fixtures and an in-memory IPC sink.
It emits only JSON measurements and a Markdown pass/gap table; it has no screenshot
writer. No board, native bridge or queue/policy operation executes.

The [historical table](matrix.md) keeps 304 source-labelled cells from the #1442
baseline. The [current table](current/matrix.md) freshly checks 304 cells: 19 routes
× light/dark × 16/20 CSS px × 320/360 CSS px × fine/coarse pointer emulation.
The eight named A1 surfaces are included. All current cells have a measured gap
in header, targets or type; there were zero page errors. A cell pass would mean
only the measured header/target/type/overflow/scroll checks passed, not all 38
rules or native acceptance. Unmeasured behavior stays unknown (R12).

[Historical observations](observations.json) and [current observations](current/observations.json)
retain text verdicts and interaction probes. [Eight image hashes](screenshots.json)
bind each retained gap image to its original source; these are before-fix browser
observations, not regenerated variants or after-pass evidence. Historical visual
inspection remains recorded in the prior commit, not duplicated as a viewing set.

Chromium 156.0.8078.4 runs at DPR 1 with simulated Windows branches. The fixed
600 CSS px viewport does not establish native content-fit, WebView2, WKWebView,
hardware scaling or screen-reader acceptance. Native and full keyboard/state/
composited-contrast qualification remain unknown.

After the existing Node/Playwright setup and `npm run build`, run:

```sh
node docs/design/evidence/1443/capture.cjs .taskq/review-1443/text-check
```

This writes `matrix.md` and `observations.json` in the named scratch directory,
with no PNGs. It does not overwrite the source-labelled committed evidence.
On this Windows host use `npm.cmd`/`npx.cmd`; no PowerShell execution policy was
changed. Browser binaries use `PLAYWRIGHT_BROWSERS_PATH` in OS temporary storage.

## Findings against all approved checks

**Pass** is bounded to the named observation, never whole-product conformance.
**Gap — observed** is a measured discrepancy. **Gap — evidence** means acceptance
is unproved, not that a defect was reproduced. **N/A** names its limited scope.
H/T/S/L/U identify the proposed tasks below. Screenshots show context; numeric and
interaction observations are in `observations.json` unless a source check is named.

| Rule | Verdict | Route / gap evidence | Measured value / observation | Fix or retained protection |
|---|---|---|---|---|
| V01 | Gap — observed | Workspace | Only Conversations/new are implemented; source has no Work sessions or Agent settings route. | H: explicit supported route ledger and unavailable destinations; keep new feature implementation outside this UI repair. |
| V02 | Gap — observed | Scrolled project · [G01](screenshots.md#h) | Back y changes from 45 to −4750 CSS px; permitted drift ≤1. | H: move Back into fixed leading slot; retain root/window boundaries. |
| V03 | Gap — observed | Settings · [G04](screenshots.md#s) | Header has 7 buttons, 0 route titles; Back and title are separate content rows. | H: one Back/title/action header, accessible overflow, separate emergency/window actions. |
| V04 | Pass — browsing scope | Workspace | Long conversation uses its own root; browsing uses push state; existing browser checks retain two drafts. | H/U: preserve drafts and non-applying exits; modal behavior is separately N/A below. |
| V05 | Gap — observed | Settings | Navigation leaves `about:blank` unchanged; recreating the WebView starts at Home; route state is an in-memory stack. | H: native route persistence/declared supported entry handling under #1440; no invented URL scheme or filesystem URLs. |
| V06 | Gap — observed | Projects | Projects → Back focuses H1 Home, not the invoking Projects button; code retains scroll/disclosures but no invoker identity. | H: restore invoker or surviving-item fallback with filters/scroll. |
| V07 | Gap — observed | Unavailable workspace | Focused Refresh → received update leaves `document.activeElement` at BODY; button lacks a retained focus identity. | U: preserve active control across changed updates, retaining draft/caret checks. |
| V08 | Pass — current browser scope | Current project | Baseline comparator stopped at a stale heading assertion. Upstream #1449 repairs it; the current shared suite passes independent project/issue/session destinations. | L: retain the repaired assertions and independent link targets; no local comparator fix remains. |
| V09 | Gap — observed | Home, coarse · [G03](screenshots.md#t) | Fine Home 25.67×27.59; count height 28; grip 18×18. Coarse Home 25.67×44, below 44×44 width. Workspace history is 142.61×21 at 16 px. | T: measure effective labelled hit areas; enforce both dimensions, spacing and nonoverlap without shrinking text. |
| V10 | Gap — evidence | Home | Product 32/44 minima fail; no per-control WCAG exception ledger was supplied. Native controls/spacing exceptions cannot establish the product minimum. | T: record applicable WCAG exceptions separately; none waive TaskQ dimensions. |
| V11 | Gap — evidence | Home | All 7 header buttons have names/tooltips; the complete enabled icon/focus inventory is not keyboard-qualified. | T: complete names/tooltips/focus pass including resize and AI history. |
| V12 | Gap — evidence | Filter | Existing filter suite passes Space/Enter and selection; no complete keyboard-only traversal of all 19 views was performed. | T: keyboard pass through all enabled controls and exits; retain existing keyboard checks. |
| V13 | Gap — observed | Workspace · [G08](screenshots.md#u) | Refresh focus is lost to BODY; Back restores H1 rather than invoker. | H/U: stable focus identity/order and visible focus after navigation/refresh. |
| V14 | N/A — web modal scope | Stop | Captured views use pushed pages/disclosures, not a web modal; native picker/sheet focus was not exercised. | Preserve this boundary; any future modal requires its own trap/exit/restore checks. |
| V15 | Pass — fixture/source scope | Stop, recovery | Missing review capability disables consequential confirms; target/effects and unknown state are explained. Existing answer/workspace checks bind confirmation to exact revisions and distinguish partial readback. | U: retain receipts, disabled unknown actions, read-only review and exact-revision confirmation. No live effect acceptance claimed. |
| V16 | Gap — observed | Project · [G05](screenshots.md#l) | Alpha appears as route title and project identity; ask #841/title repeat in the task block and card. Board also repeats lower on the project page. | L: render one identity/Board target and one task/card heading; keep independent destinations. |
| V17 | Gap — observed | Empty Home · [G06](screenshots.md#l) | With 0 ask/failed tasks, Home retains Attention + All as a row. Direct empty Attention does explain its state in source. | L: omit empty summary rows, retain explicit direct-empty explanation and urgent-first order. |
| V18 | Pass — fixture scope | Filter | 180-character titles use full-width continuation; measured scrollWidth equals 320 at the narrow boundary. Metadata and title remain adjacent flowing lines per M3. | L: preserve full titles and atomic identifiers; no new columns or truncation. |
| V19 | Pass — current R6 fixture scope | Current strip, 1000 summary counters | Summary counts 0 omit capsules; the current top strip retains all eight states with icon-only zeros and explicit unknown. All tasks reports 22 deduplicated open fixture tasks. | L: retain both zero policies, separate Doing/Review, open total excluding Closed, and unknowns (R6/R12); align stale v2 wording. |
| V20 | Gap — observed | Home · [G04](screenshots.md#s) | Tray content inset is 6 px vertical/8 horizontal; group margin and padding are 5 px each, against 6 px section target. Workspace uses 12 px insets without a shared declared surface token set. | S: declared per-surface tokens, one 1 px divider/6 px section treatment; do not impose tray dimensions on workspace. |
| V21 | Pass — current component scope | Current counters | Current shared comparator passes 64 parent/browser/tray comparisons and all wrapped right edges at 0/1/12/100; the 1000 probe fits 304 px. Native/all-control collisions remain unproved. | L: retain K-A8/right-edge checks and distinct total/project scopes; T owns effective target collisions. |
| V22 | Gap — observed | Settings · [G04](screenshots.md#s) | Tray H1 computes to 20 px/700, versus target 22 px/600 at b=20. Workspace title is 22 px/700. | S: system type roles at b and 1.1b semibold; retain secondary floors and independent icon calibration. |
| V23 | Gap — observed | 200% text · [G02](screenshots.md#h) | At 320 px, 40 px text puts Close x=329.31, width=43.38, outside the viewport; document overflow=0 because content clips. | H/T: reachable essential header controls and overflow at 200%; test rectangles, not only document scrollWidth. |
| V24 | Gap — observed | Light counters · [G07](screenshots.md#s) | Light Closed text `#68727d` on opaque `#eff1f3` computes 4.32:1, below 4.5:1. Other composited/disabled states remain unqualified. | S: correct quiet-text token and measure both themes/all states; preserve non-color semantics. |
| V25 | Gap — evidence | Workspace light | Input edge `#ccd3da` vs `#fafbfc` is 1.46:1; dark `#53616e` vs `#252b32` is 2.25:1. Whether each boundary is essential/excepted is not documented. | S: qualify essential graphics at ≥3:1 and document valid exceptions; do not claim WCAG failure solely from a decorative edge. |
| V26 | Gap — observed | Scrolled project · [G01](screenshots.md#h) | One main tray scroller exists, but Back leaves it; fixed 600 px captures cannot establish native natural-fit or usable-area sizing. | H: fixed navigation; retain one intended main scroller. Native content-fit acceptance remains separate. |
| V27 | Gap — evidence | Unavailable workspace, Agents · [G08](screenshots.md#u) | Unknown capability, unavailable service, partial/Closed-unknown and empty probes exist; full normal/loading/no-match/stale/failed matrix across all routes is not complete. | U: named state fixtures with reason/recovery, never default unknown to zero/healthy (R12). |
| V28 | Pass — bounded fixture checks | Workspace | Existing answer suite passes same-ID retry, stale review, pending and accepted readback; workspace suite passes double-click suppression and partial outcomes. | U: retain those assertions and exact revision/receipt semantics during migration; no live execution claim. |
| V29 | Gap — observed | Workspace · [G08](screenshots.md#u) | Workspace contains 0 status/live regions; routine transcript/status updates have no programmatic status announcement. Errors have role=alert, which does not cover routine status. | U: polite non-focus status announcements; do not move focus or announce every refresh urgently. |
| V30 | N/A — superseded local disclosure | Current filter | Current R6/K-A6 removes the checkbox disclosure; counters toggle with Enter/Space and the result list stays direct. Baseline disclosure pass is historical. | L: align the v2 local parameter/preservation map to R6; retain direct keyboard-operable filters, not checkboxes. |
| V31 | N/A — superseded select-all checkbox | Current All tasks | No aggregate checkbox exists under current R6. Capsules expose aria-pressed; All tasks clears project/state restrictions. | L: retain pressed-state accessibility and All tasks semantics; no mixed-state checkbox restoration. |
| V32 | Pass — current browser scope | Current All tasks | Current suite passes OR toggles within project scope, clearing last state = unrestricted, zero states, Reset and global All tasks. | L: preserve accepted set operations; Reset clears selection and leaves execution policy unchanged. |
| V33 | Pass — current browser scope | Current filter | Current suite passes Back/refresh retention, zero-match Reset, 6 deduplicated open test tasks across projects and no policy writes. | H/L: preserve view-only semantics; old single-state replacement behavior is superseded by approved toggles. |
| V34 | Gap — observed | Projects, Settings · [G04](screenshots.md#s) | Add project is after project rows rather than beside the section heading; Settings children are text-only rows without purpose icons. | L: place add/link actions beside subjects, group Settings by purpose with recognizable icons/labels. |
| V35 | N/A — spatial animation scope | Workspace | Reviewed tray/workspace source defines no spatial animation/keyframes/transitions to disable. Disclosure arrows switch without animation. | Keep no-bounce behavior; native motion is unknown and outside this browser observation. |
| V36 | Gap — evidence | Home | Normal/empty/unavailable views captured; no complete normal/blocked/critical/frequent-use refresh sequence with an active pointer/keyboard target. | U: explicit state-update sequences; keep essential controls stable without frequency-based rearrangement. |
| V37 | Gap — observed current document conflict | [V2 profile](../../ui-rules-v2.md), [current counter contract](../../task-counters.md) | V2 still requires no zero capsules, a checkbox disclosure/mixed select-all and old shortcuts; current R6/K-A2/K-A6 requires top-strip zero icons and toggles. | L: repair local v2 parameters/preservation mapping to the already approved R6 decision; no new owner policy decision or shared-rule change is implied. |
| V38 | Gap — evidence | [Route matrix](matrix.md) | All concrete route/view cells are checked in the text tables; owner visual acceptance, native keyboard/accessibility and corrected full return/error flows remain unproved. | Each fix supplies its own measured before/after plus actual-size component/route review before reuse (R12/§ 7.4). |

Contrast figures use the sRGB relative-luminance formula on the opaque source
tokens named in the row; they are not a complete composited-color audit.
Targets use the associated clickable label for checkboxes, exclude closed
disclosure contents, and retain offscreen controls in the raw inventory for
reachability analysis. A target inventory is not proof that every rectangle is
reachable or nonoverlapping. The 200% probe demonstrates why scrollWidth alone
would falsely pass clipped header actions.

## Proposed fix tasks and order

These are **draft briefs, not filed tasks**. The owner selected Preact + Signals +
Wouter in [#1440](https://github.com/taskqorg/taskq/issues/1440#issuecomment-6309805829).
At this review's board read #1440 was still doing; its accepted specification,
route/store ports and foundation migration handoff are a prerequisite, not a
shipped dependency. Do not build a second vanilla router/store or migrate shared
framework-free project blocks to the framework. Keep native persistence, IPC,
snapshot ownership, permissions and effects intact.

Size is a planning estimate: S = one component/token diff; M = one coherent
cross-route behavior diff. If the foundation handoff makes a brief larger than
one reviewable diff, split by complete user scenario before filing (R2/§ 7.1).

| Order / draft title | Rules | #1440 foundation and scope | Size / real prerequisite | Observable acceptance and preservation |
|---|---|---|---|---|
| [1. H](screenshots.md#h) — Keep route identity and Back reachable | V01–V08, V13, V23, V26 | Preact shell, Wouter memory route ledger, Signals view state; native route persistence through the agreed adapter. Include explicit supported/unavailable entry states; workspace stays its own root. | M; foundation handoff. | One fixed Back/title/action header; ≤1 px Back drift; child/deep-entry return; reload and filter restoration; invoker focus; Close/Back reachable at 320 px/200%. Preserve native chrome and emergency distinction. No new Work sessions/Agent settings backend. |
| [2. T](screenshots.md#t) — Make every input target usable | V09–V14, V23 | Shared Preact controls use H's narrow overflow slot; existing project/answer components retain their APIs. | M; H only for the overflow slot. | Fine/coarse ≥32×32/44×44 effective targets with nonoverlap; keyboard activate/exit, tooltips/names/focus; exceptions recorded separately. Keep width, text size, action semantics and independent issue/session/Board links. |
| [3. S](screenshots.md#s) — Apply spacing, type and contrast tokens | V20, V22, V24, V25 | One CSS token layer consumed by Preact tray/workspace, with explicit per-surface values; Signals font preference uses native persistence. | S; foundation handoff; order after T is coordination, not a hard dependency. | Exact local spacing/type roles at 14/16/18/20; 4.5:1 text, 3:1 essential graphics or documented exception; preserve independent summary icon sizing. No new theme preference or platform scaling policy. |
| [4. L](screenshots.md#l) — Remove repeated list content and preserve filters | V08, V16–V19, V21, V30–V34, V37 | Existing framework-free project/answer blocks mounted through #1440 component ports; filter/view state uses the agreed Signals store. | M; foundation handoff; coordinate shared target/token edits with T/S. | First align stale v2 clauses with R6. One identity/Board target and task/card heading; empty summary omitted; inline Add; Settings purpose groups; full-width titles; right-aligned counts. Keep zero-icon top strips/nonzero summaries, All tasks open total/project grouping, OR toggles/aria-pressed, Reset/Back/no-write checks and upstream comparator repair. |
| [5. U](screenshots.md#u) — Preserve focus and explain every update state | V07, V13, V15, V27–V29, V36, V38 | Stable Preact node identity and Signals subscriptions over existing native snapshots/receipts; no query cache or new effect executor. | M; foundation handoff; use H's focus identity contract. | All eight display states have reason/recovery where relevant; focused refresh/control and drafts/caret survive; polite status announcements; same-ID retry and exact revision/readback remain; normal/blocked/critical updates preserve active targets. |

All briefs retain R6 data meanings, R9 authority, R11 active work and R12 unknown
semantics. Ordering is a recommended execution sequence, not five artificial
blocking dependencies. H owns shared route/focus identity; U owns updates. T owns
hit-box/keyboard behavior; S owns paint/type tokens; L owns list composition.
The five writers must not edit the same uncommitted shell/shared files in parallel.

Common delivery: reuse this source/hash-pinned baseline; reproduce the named
failure, change only the accepted behavior, rerun that check for an after-pass,
run the selected project gate, update affected source/fixture evidence, and obtain
owner actual-size visual acceptance. Do not call browser proof native acceptance.
GitHub publication uses green merge-result `merge_group` CI; private fallback uses
exact-SHA CI. This proposal neither merges a PR nor claims those gates passed.

## Checks and remaining work

- `npm.cmd ci --ignore-scripts`; `npm.cmd run build`: passed; source bundle generated.
- Updated `node docs/design/evidence/1443/capture.cjs .taskq/review-1443/text-check`: 304 checked cells, 0 pass / 304 measured gap, zero page errors, zero PNG output. Self-checks cover pass, undersized-target and scrolled-Back classification. This replaces screenshot-per-cell output under updated A1/A2.
- `node tests/tray-state-filter.browser.cjs`: passed; selection, keyboard, scope, Back/refresh and no writes retained.
- `node tests/workspace.browser.cjs`: passed; drafts, full replies, IME, exact revisions, double-click and partial-outcome checks retained.
- `node tests/tray-answer.browser.cjs`: passed in both themes at 16/20 px; unchanged DOM, draft/caret/focus, stale review, same-ID retry and accepted-readback checks retained.
- `node tests/project-blocks.browser.cjs`: **failed** at `tests/project-blocks.browser.cjs:71`. Expected `/project-with-a-long-name.* · Review/`; actual heading `Tasks`, with separate `Filter: Review`. Product source and assertions were unchanged. This blocked the baseline V08/V21 pass; the upstream before/after check is recorded next.
- After rebase, current `node tests/tray-state-filter.browser.cjs` and `node tests/project-blocks.browser.cjs`: **passed** at product source `8669a08ec2435e830301bf7307fb37f477c87e6d`; the original 80 changed-route checks completed; their screenshot variants are removed. The heading repair and toggle behavior came from upstream #1449, not this task. Baseline failure remains above; its V08/V21 limitation is resolved by the current bounded pass. Other focused results cover unchanged workspace/answer paths. Full native/merge-result acceptance is not implied.
- `python3 scripts/check.py --base 8bf66914f03560c8c3f979523dce7d6781fe5420`: **failed**, full selection; Rust 427 passed, 21 failed, 36 ignored; 322.562 s / 900 s. [Failure inventory](checks.md). Host tests refused by worker authorization are `sandbox-blocked`; other failures remain unqualified. The gate stopped before offline selftest and its Node stage. Worker environment and all assertions were retained; no permission change or retry.
- `node --check docs/design/evidence/1443/capture.cjs`; report/matrix local links; 38 unique ordered verdicts; eight retained image hashes; PNG cap and zero-output readback; `git diff --check`: passed. Product source and regression assertions are unchanged.

The first extended capture attempts used an incomplete history/notification
fixture; source-owned period/notification fields were then supplied before the
complete run. Those harness failures do not establish a product defect.
All original regression assertions are retained. No new production logic or CI
gate was added; the capture is a research instrument under R12.

Owner decision: approve all five briefs in the listed order, approve H+T first and
defer the rest, or revise named briefs. No dependent task is filed before that
answer (§ 7.5, A4). UI fixes, after-pass evidence, native qualification, full
keyboard/contrast/state coverage and foundation readiness remain open.
