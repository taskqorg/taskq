# Conversations candidate · #1580

Workspace drafts, files and request history are implemented; full dialogue stays in the native session. This is builder evidence, not owner visual acceptance, a merge or deployment.

## Candidate and authority

- Product decision: [owner answer 2](https://github.com/taskqorg/taskq/issues/1580#issuecomment-6343852087) amends A1/A7 to native-only dialogue, workspace drafts/files/history and Open session. No in-workspace reply chain, operator/proposal protocol or native transcript store.
- Applied rules: R3 native owner sessions; R8/R13 specification and implementation together; R9 unchanged grants/runtime policy; R12 qualified evidence; §4 local draft/view metadata; §7 immutable candidate and publication.
- Source/contract/spec/fixture/image hashes: [candidate.json](candidate.json), [render manifest](rendered/manifest.json). Shared rules: specq `d67126cd7e4fb88bcabd63de10748c5c3a9f5442`.
- Frozen reference: [#1573 package](../frozen-1573-package.json), `control-panels.html` SHA256 `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`. Downloaded ZIP matched SHA256 `d9f39f3f24bbb4e43d94c113dbdd222bf839ee78e67e86de4fa0b12e2ef1f640` (7,839,451 bytes). Answer 2 supersedes its dialogue chain and active-only archive restrictions.
- Environment: macOS arm64; Playwright Chromium headless, system type, scale 1; six pinned browser fixtures at 16/20 px, dark/light. Native README capture uses isolated WKWebView. Conversation monitor usable rectangle, native chrome/DPI and screen-reader behavior: Not tested.
- Builder reviewed all six final images at natural size. No second agent review or owner acceptance is claimed.

## Feature and navigation ledger

Pass below is limited to the named layer. Fixtures use invented registered projects and explicit native receipts, with no board or production launch effects. Real filesystem/setup tests use isolated stores and a substituted native launcher.

| Requirement / route | Initial → success → unknown/failure → recovery | Evidence / result |
|---|---|---|
| A1/A7, C1/C2, C-A1: Conversations → New → select | Empty → independent text/context/caret/files/name → missing project/storage → preserved draft, explicit same-ID check | Browser and native filesystem Pass; replies remain native |
| A2/A3, C2/C7, CW1–CW8: Recent / Archive / History | New clears search and reveals its row → explicit title/list selection/scroll → no matches → clear, Undo, Restore; History fills only | Browser Pass, including 60-item list, reload, close and two independent drafts |
| A3, C2/C-A2: Detach / Return / reopen | One editable owner → detached owner → main read-only → raise or Return; close never sends | Browser ownership protocol Pass; native window routing/source implemented; real OS click/raise Not tested |
| A4/A5, C3/C-A4: Attach / drop / preview / remove | Selected bytes → owned copy/hash/length → invalid type/oversize/storage/hash mutation → preserve other files and text | Browser FileReader and native filesystem Pass; real runtime Read delivery Not tested |
| A5/A6, C4/C-A5–7: Send / Check delivery / Cancel | Freeze revision and ID → one setup record → lost/partial receipt stays unknown → read original ID; pending cancellation acknowledged under setup lock | Native filesystem/setup and browser Pass; real runtime launch/readback Not tested |
| A6, C7/C9: next draft / archive / permanent delete | Independently edited next text/files → separate unsent ID → copy failure refuses delete → readback; sent history never resurrects | Native filesystem and browser Pass, including identical retyping and reused destinations |
| A7, C5/C6/C-A3/8: Work sessions / Inbox / review | Recorded worker/reviewer target → verified local URL → remote/missing target reason → return without steering/reply bridge | Browser Pass; existing exact-answer and operation-receipt guards retained |
| A7, C-A9: existing setup/plans/resources/delivery routes | Named project and native owner sessions/current reviews → existing operations → explicit unsupported remote/headless capabilities | Existing browser instruments Pass; no new gateway, grant, scheduler or remote adapter |
| A8, C-A10: fixed chrome / composer / keyboard | 16/20 px, two themes → visible Send/Attach/Back → narrow/short/long name → wrapping and separate record/list scroll | Browser geometry, keyboard/IME event simulation Pass; real IME/screen reader/Windows native DPI Not tested |

Board questions and reviews remain the existing Inbox/exact-revision path. Selecting a recorded session opens its supplied R6 capability; it does not inject a reply or infer runtime completion from `launched`.

## Findings and retained protection

| Defect before | Fix and pass after | Protection retained |
|---|---|---|
| [Original bundle](before.txt): CW1 found 0 direct New controls | Actual-bundle CW1–CW8 Pass | No selection/New/History/reload/close launch |
| [Prior candidate](before-draft-transfer.txt): New from draft lost file metadata | Native copy/revision/source-destination binding plus browser transfer Pass | Same immutable sent request; no automatic launch, replay or sent-history recovery |
| Deleting a sent record could discard an independent next draft | Preserve independent text/new files before deletion; four native tests Pass | Original source files/native chats untouched; failed copy refuses delete; tombstones prevent resurrection |
| [Narrow before](narrow-before.png): long file name hid Attach/Send at 350×560, 20 px | [After](rendered/unavailable-narrow-light-20.png): wrapping and fully visible controls; geometry assertions Pass | File identity and full name remain available |
| [Selection before](selection-before.png): selected row had no house highlight; dense New row stayed offscreen | [After](rendered/draft-files-dark-16.png): house current-page style; 60-item regression Pass | Independent Recent/Archive selection and scroll |
| Loading all workspace controllers exhausted CoreQ's 256 method slots | Register consumer methods on first use; 300 unused methods and cross-route operation regression Pass | Existing context, receiver, thrown error identity and 10,000-event coalescing tests retained; no dependency/policy change |
| Existing tests expected the removed Conversations alias / Work sessions placeholder | Expect approved New/real supplied targets; browser Pass | Original Inbox deletion, exact Full stop/unknown receipts and navigation/focus guards retained |

## Verification

See [candidate.json](candidate.json) for exact candidate check outcomes. The canonical check is **not Pass**: worker host-authority tests are refused; integration contains a sandbox-exec denial. These restrictions were not bypassed or weakened. No fixture result qualifies production or another platform (R12).

- `python3 scripts/check.py --base c373c0ccf78567fa2b3becaad27307b7eec32f41`: sandbox-blocked, 488 passed / 18 failed by worker authority guards / 34 ignored. Four Conversations tests passed.
- `cargo +stable test --locked --test integration --no-fail-fast`: sandbox-blocked, 19 passed / 1 sandbox-exec refusal / 1 ignored.
- Native draft/file tests: four meaningful filesystem/setup cases cover stale revisions, altered bytes, corrupt/unavailable/link storage, budgets/types, archive/restore/delete fences, immutable delivery/reconciliation, acknowledged queued cancellation and independent next-draft preservation.
- All 21 browser instruments passed in the final complete run after rebase and list-position correction. Real browser actions use the actual generated workspace bundle.
- All `tests/*.test.cjs`, release tag/check scripts, screenshot guard fixtures and UI inventory check passed. Existing protection was not deleted to make the new interface pass.
- Offline staged selftest: 97 golden traces and format checks Pass; private telemetry capture committed 9 observations. Board read is unknown because offline mode was explicit.
- README shots rebuilt with `docs/shots/update.sh`, then `docs/shots/check.sh`; native source/blob provenance is separate from browser fixtures. The project and summary shots were visually inspected.

## Scope, limits and release gaps

PDF is explicitly unsupported (0 pages). Selected UTF-8 text/Markdown and PNG/JPEG/WebP are limited to 4 files, 5 MiB each, 20 MiB per draft, 100 MiB owned copies, 8000 px per image side; text 64 KiB and metadata 8 MiB. No file content enters command history/telemetry. No source-path deletion, native-chat scan or runtime Stop is added. Cancellation covers acknowledged pending launch only.

Windows native renderer/DPI, actual OS picker/drop, real native IME/accessibility, detached-window lifecycle and Claude Read delivery were not tested. Native click/raise proof is owner-observed under UI acceptance and does not block this task. Changed-runtime qualification remains the owner release batch under §7.8; no live installation, release, permission/capacity edit or deployment occurred. Merge-result CI and review remain publication gates (§7/R12).

Rollback code through the ordinary reviewed release path; preserve local request/attachment stores. Older UI cannot expose the new drafts, so a code rollback alone is not evidence of draft recovery. No migration deletes existing request/answer/history or unknown preference keys.
