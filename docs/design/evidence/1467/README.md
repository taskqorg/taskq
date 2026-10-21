# Messages and permanent archive deletion · #1467

This candidate implements TW5–TW7 under the [owner's message-only answer](https://github.com/taskqorg/taskq/issues/1467#issuecomment-6320097999). Native chats remain native, pending/launching requests refuse deletion, and #1464's attachment handoff deferral remains. No board write, worker cancellation, runtime fallback or permission change is added (R1/R9/R11/R12/R13, § 7).

## Acceptance and retained protections

| Item | Evidence and limits |
|---|---|
| A1 | The tray Messages count and workspace consume the same native snapshot. Eight Rust message-boundary tests cover duplicate board questions, read versus unanswered, new revisions, unread launch outcomes, archive exclusion, corrupt input and stale sources reading unknown. The badge adapter retains explicit unknown/count checks. |
| A2 | The real bundled browser opens existing records with only a local read mark; no send or answer is dispatched. Back retains filter and invoker focus; restart retains the filter, and composer drafts survive archive. The existing workspace communication instrument retains draft/caret, IME, Shift+Enter, same-ID recovery and answer-revision coverage. Attachments stay deferred; browser events do not qualify physical IME or native session behavior. |
| A3 | Any request state and unresolved question can be archived without a process action. Archived source identities remain accessible after a launch state change or board answer, without copying board text or native replies. A new outcome/revision has its own identity. |
| A4/A5 | Native deletion removes only an archived owner's request record, temporary write copy and owned setup folder. Questions lose local metadata and keep only a content-free suppression digest; the board stays authoritative. Request-ID digests prevent stale clients from launching deleted work again. Tests preserve source/native markers, refuse Windows junctions and prelaunch states, and verify owned-path absence. This is model/filesystem evidence, not a live Claude turn qualification. |
| A6 | Real filesystem tests cover failed/partial deletion, fresh-store readback, archive aliases, metadata corruption, interrupted launch recovery and a late callback. The same setup lock serializes launch and deletion. Each Delete all item receives its request's verified outcome. Unix symlink coverage is retained in the same module but is not run by this Windows host. |
| A7 | Archive, permanent delete, Close workspace and command history are separate controls. The browser checks keyboard Enter/Back/Escape, no resend/cancel/history-clear effects, unknown feedback and a narrow 20 px view. Request/answer receipts and drafts keep their existing adapter. |
| A8 | The four browser images below show before/after, archive consent and narrow unknown/partial states. The instrument also checks the reachable last action and fixed navigation in a 360×640 window; the narrow review image uses 360×880 to frame all feedback. Native captures are separate; neither substitutes for native workspace qualification or owner visual acceptance. The combined seven-point review below records unfinished owning tasks explicitly. |

## Before and after

- Before `node tests/tray-home-ai.test.cjs`: failed because the Messages badge renderer was absent. After: the same check passes with count, destination and unknown reason assertions.
- The first browser implementation could not dispatch Read because `crypto.randomUUID()` was unavailable in the renderer context. The existing `getRandomValues` pattern fixes it; the message browser instrument passes opening without resend, Back/filter restoration and archive/delete feedback.
- Both safety-review regressions failed before their fixes: an existing `messages.tmp` hard link was accepted, and two archived states of one request produced a false unknown receipt. After: all eight message tests pass. Exclusive temporary-file creation preserves linked targets; one deletion result covers each selected alias.
- The staged-history assertion also failed before deletion removed `setup/<id>.tmp`. After: the same test verifies the temporary brief copy is absent as well as the main record and folder.
- R7 receipt wording: the browser assertion failed on `archive read back for 22222222`. After: the same assertion passes with `Message archived`; read/delete use plain action labels too, while per-item refusal reasons remain visible.
- The publication budget test failed with two extra issue reads on the other host. Its shared `/nonexistent` installation allowed unrelated profile rows into the test. A unique temporary installation fixes the failure while retaining every exact read-count assertion. The focused test passes.
- Before rebasing, the corrected full gate reported **sandbox-blocked**: 444 passed, 19 worker-authority failures, 35 ignored. The final combined candidate rebases onto `7cc123790fd1fd5612a112fb05abe3c777840f09`, retaining the incoming CoreQ fix and shared task controls. `python3 scripts/check.py --base 7cc123790fd1fd5612a112fb05abe3c777840f09` took 68.453 s on this Windows GNU host: 444 passed, 20 failed, 35 ignored. Nineteen failures remain worker-authority refusals; `runtime::app_control::tests::cancellation_is_owner_bound_bounded_and_never_replays_work` failed with a native broken pipe. An earlier rebased full run timed out in the native pipe-bounds fixture, which passed alone. These full-run native failures remain open; no deadlines, assertions, worker identity or authority guards were weakened. Merge-result CI remains required (R12, § 7).
- After rebase: all Node component/IPC checks, both affected browser instruments and screenshot/release-script checks pass. Integration: 18 passed, 1 ignored. Isolated offline selftest passes: 97 fold traces, formats and staged telemetry commit only; it reads no board and does not establish live board health.
- Check identity: the full Rust gate, integration and offline selftest above ran at `8f91dcb6c6209ced681bc3d3239f3c735d99f0c5`. The subsequent `a0c1db68d5dd35ddaced64b8fbfe60507677a2e4` change only replaces receipt wording; its affected browser/badge checks and refreshed native captures pass. Rust behavior, tests and dependencies are unchanged. Later evidence commits change documents/images only; the final publication SHA still requires CI.
- Native Windows capture: the direct GNU executable invocation exited 127; running the same release through Cargo passes and preserves its build loader environment. `docs/shots/update.sh` now supports Windows paths/metadata and keeps Cargo's configured macOS signing runner. Rebased native captures passed on Windows; macOS was not rerun here. The separate Home capture uses invented source files, shows `Messages · 2`, and launches no session.

The local metadata writer refuses a `messages.tmp` left by a crash; it does not overwrite or interpret that file as recovery data. An operator must resolve the stale temporary path before further metadata writes. Unknown remains visible. Deleted message history has no undo, trash or retained content copy; command history and drafts remain distinct under the approved boundary.

## Combined seven-point review

| Owner point | Current result and owning task |
|---|---|
| 1 — navigation | #1463's shared fixed bar remains in both windows. Messages is a root with no cross-window Back. Message details use Back and restore the list's filter/focus. |
| 2 — dialogue placement | #1464 supplies workspace requests and task answers. Consequential operation reviews remain unfinished in #1465, observed doing during this review; this candidate does not move that task's controls. |
| 3 — panels before tasks | #1466 owns the remaining compact panel/order work and was observed parked (`later`). Messages is added to the current Home surface; this is not acceptance of the final panel composition. |
| 4 — other content | Messages and archive live in the workspace; native replies remain in Claude/Codex by the accepted scope. Existing Work sessions/Agent settings fallback screens still name the tray destination; this task does not implement those separate screens. |
| 5 — messages/archive/delete | Implemented with stable identities, explicit unread/unknown handling, archive-any, archived list, permanent one/all deletion and the prelaunch refusal. |
| 6 — whole-surface review | Source/route review completed; final combined visual/interaction acceptance remains open until #1465/#1466 integrate and the owner reviews the resulting surface. No synthetic fixture qualifies native workspace behavior. |
| 7 — interaction consistency | Existing source option IDs and direct-answer semantics stay in the shared answer card. Message list selection is a labelled select; actions use ordinary buttons, with no selected-action styling. Keyboard and receipt behavior are covered by the bundled browser instruments. |

## Visual review set

Seven-image set: four browser images, one native Home image and two native README images. Fixtures are invented; browser images use Chromium headless shell on Windows at actual CSS sizes without resizing. WebView2 images retain their captured physical pixels. The native binary and source blobs are recorded in `docs/shots/manifest.json`; source and image hashes are recorded in `sources.json`.

Native Home, including the Messages badge (the unfinished panel order remains #1466's gap):

![Windows WebView2 Home](native-home-dark-16.png)

Native README [project capture](../../../shots/popover-dark-project-16.png) and [icon capture](../../../shots/icon-summary-run-dark.png). Before: [project](https://github.com/taskqorg/taskq/blob/2f6a2d2c9ac427f9687b64714bd2cfd21805773c/docs/shots/popover-dark-project-16.png), [icon](https://github.com/taskqorg/taskq/blob/2f6a2d2c9ac427f9687b64714bd2cfd21805773c/docs/shots/icon-summary-run-dark.png). Including those two earlier images keeps the review set at nine, below V38's cap.

Before, baseline `2f6a2d2c9ac427f9687b64714bd2cfd21805773c`:

![Before: questions only](messages-before-dark-16.png)

After, Messages and pending outcomes:

![Messages, dark, 16 px](messages-dark-16.png)

Archived records and permanent deletion boundary:

![Archived, dark, 16 px](archived-dark-16.png)

Narrow unknown source and a refused prelaunch deletion; the content has one intended vertical scroll area:

![Partial deletion, light, 20 px, 360 wide](partial-light-20-360.png)
