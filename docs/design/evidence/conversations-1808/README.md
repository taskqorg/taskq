# Conversations sidebar · #1808

The owner [selected Inline actions](https://github.com/taskqorg/taskq/issues/1808#issuecomment-6490535339). [CS1–CS6](../../tray-workspace.md#conversations-sidebar-1808) specify grouping, ordering and local actions; the [usability review](../../conversations-usability-review.md#sidebar-grouping-1808) records the scenarios. Source implementation exists in the draft PR. It is not merged, deployed or native-qualified.

**Publication blocker (R13):** the rule gate identifies the root-contract amendment as `Product`, but #1808 and its answer do not name that exact ID. `python3 scripts/check.py --rules --base c761cc2ed608b26638bc295dc5f3e5a6690ffb68` refuses with `the task does not name Product (changed)`. The selected layout is settled; the remaining owner ask explicitly names the proposed Product sidebar clause. No gate was weakened or bypassed.

## Changed behavior and boundaries

- Owner, task and event rows share Pinned → projects → Other, unique membership and descending source activity. Project groups sort by their newest unpinned member. Unknown timestamps sort last with a reason; tie order uses identity. Launched chats retain their original project when the next-draft destination differs. Ambiguous or unavailable project identity is never guessed.
- Project headers show counts and persistent collapse. Search temporarily reveals collapsed matches and restores collapse when cleared. Keyboard focus, selected identity, drafts and reading positions survive ordering updates. Inline Pin/Unpin and Archive/Restore are keyboard-accessible and visible on touch.
- Pins, collapse and eventless-task archive use content-free metadata in the existing locked preference store, separate from bounded navigation. Writes validate identity and shape, read back, report failures and never replay uncertain receipts. Owner/message archives retain existing native receipts and deletion guards. Local organization also works when editing belongs to a detached window; draft ownership remains fenced.
- Task recency comes from trusted, unedited board comments or issue creation, not labels or observation time. Draft content changes have a stored timestamp; caret, scroll, pin and archive do not manufacture activity. Native recency uses the latest visible transcript record, including fractional timestamps. Sidebar probes run one at a time on the existing background read channel; no synchronous transcript scan was added to the tray snapshot. Missing native times remain unknown, including unqualified Windows transcript paths.
- Sessions, attempts, task state, native chats and source files are untouched by organization actions. Existing launch, reply, files, history, Archive/Restore, deletion, watch and source/revision guards remain in their existing owners.

## Review set

Stage 1 is the historical six-screen design set. The current set has four browser screens plus the two mandatory README icon captures below (six images, within the review-set cap). No image qualifies an installed host.

Stage 1: [before](00-before.png), [Menu comparison](01-menu.png), [selected Inline reference](02-inline.png), [collapsed search](03-search.png), [Archive/Restore](04-archive.png), [narrow List/Back](05-narrow-light.png). The [editable comparison](../../exploration/conversations-sidebar.html) embeds the v7 bundle; its data and effects are isolated.

Stage 2, actual production bundle with invented native receipts:

1. [Inline grouping](implemented/01-inline-groups.png).
2. [Search across a collapsed group](implemented/02-search.png).
3. [Archive with ordinary task threads](implemented/03-archive.png).
4. [Narrow list and keyboard actions](implemented/04-narrow-list.png).
5. Required README icon refresh: [before](implemented/05-readme-icon-before.png) and [after](implemented/06-readme-icon-after.png), each 68×68. `docs/shots/update.sh` captured the isolated WKWebView/renderer output from `c8af96a3742fde9aca6e3fe200bce8cc1b9eba42`; `docs/shots/check.sh` confirms every UI source blob. The generated icon fixture differs while the icon renderer is unchanged; the cause was not investigated in this sidebar task. Owner acceptance of this generated refresh is included in the Product ask.

Unchanged: workspace chrome, toolbar destinations, Details, reading area and composer; bundled Lucide assets and the accepted palette/type scale. Intentional differences are the CS1–CS6 headings, ordering, counts, row actions, search and wrapping. The frozen reference is untouched. Final browser images were inspected at actual size; source rendering is distinct from native acceptance (R12).

## Checks

Environment: local macOS, Playwright Chromium headless shell; actual-bundle geometry uses 1100×850 and 500×850 CSS px, 16/20 px, light/dark (8 cells). Stage 1 comparison additionally covers two action variants (16 cells).

Before: `node tests/conversations-sidebar.browser.cjs` failed `CS1: all three sources share ordered groups`, observing `[]` instead of Pinned/Beta/Alpha/Other. After: the same check passes mixed-source identity/order, pin/unpin receipts and refusal, collapse/reopen/search, board and unselected-native-message reorder, original-project ownership, detached local archive, Archive/Restore including eventless tasks, retained drafts/focus, keyboard and geometry. It asserts no send, reply, cancellation, task-control or session action.

Filtered Rust protection:

- `cargo test --locked --bin taskq sidebar_`: 3 pass. Validated local metadata survives reopening; invalid writes preserve bytes and drafts; organization creates no request. Native Claude/Codex record timestamps ignore trailing progress/partial writes and remain unknown when missing. Board projection ignores observation/label time and untrusted/edited comments.
- `cargo test --locked --bin taskq conversations::tests`: 6 pass, retaining attachment/path validation, immutable launch identity, deletion and source/execution preservation.

Existing browser checks pass: `conversations.browser.cjs`, `conversations-selection.browser.cjs`, `conversations-stable.browser.cjs`, `dialogue.browser.cjs`, `workspace-communication.browser.cjs`. They retain draft/caret/files, Archive/Undo/Restore, immutable sends, source choices, scoped receipts, growing input, history, transcript paging, watch, late/failed read and reload protection. The stage 1 prototype check remains separate from production proof.

The full gate is not PASS. `python3 scripts/check.py --base c761cc2ed608b26638bc295dc5f3e5a6690ffb68` stopped after 1.765 s on the current candidate at the Product rule-ID gate; later steps did not run. The earlier stage 1 full run was sandbox-blocked after 109.157 s (574 Rust pass, 18 host/capacity/lifecycle failures, 33 ignored, including explicit worker refusals); it does not qualify this implementation. Worker identity and permission markers were kept. CI must still pass on the merge result (§7/R12).

| Acceptance / boundary | Status |
|---|---|
| A1 design package, owner visual choice | Pass; Inline selected |
| A2–A6 source implementation and listed fixture flows | Pass in browser/Rust fixtures |
| Whole affected workspace visual comparison | Pass in browser fixtures; four build images inspected |
| Product rule-ID and refreshed capture authorization | Pending; exact ID absent from task/answer; capture pair shown above |
| Canonical gate and merge-result CI | Blocked / not qualified |
| A7 installed Mac/Windows | Not tested; owner-approved §7.8 release batch |

The required source screenshot manifest is refreshed and its source check passes. After the owner answer names Product and accepts the captures, rerun the selected canonical gate on the rebased candidate, publish the result and let review/merge-group CI gate completion. No live installation is part of this task.
