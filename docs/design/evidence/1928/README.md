# Inline task inspection and navigation · #1928

Implements the existing Product/UI30, M3 and MP13 targets. The owner note adds the `merge queue · CI` presentation of Review, using its canonical queue receipt and existing publication detail. The existing task projection and writer are reused; no lifecycle state, poll, grant or task store was added (R1/R6/R13).

## Candidate and scope

Implementation candidate: `edcf7deee6d6be9ea8ac5ae0e388232cf67f958c`, rebased once onto `b0c5269e498824d208ddd04e4da86b3c0d45b45f`. The rebase retained #1948's host layout and #1949's Reports fix. Subsequent changes contain evidence and test-only selector/diagnostic corrections; application sources are unchanged.

The reference is the [Oct 30 packet](https://github.com/user-attachments/files/34184601/taskq-ui-polish-handoff-2026-10-30-v2.zip), verified as SHA-256 `e397147ee8fe6d6867af8ecfe604141e9921b0ce6c46e2f059a03851e9b3342d`. Its source-copy adapters were inspected, not shipped. P-W1/#1829 supersedes its incidental retained-closed-pin example.

| Required capability | Current destination and protection |
|---|---|
| Eight open states | `task-dashboard.js` renders one raised inline panel in the main list scroll. The parent retains title, number, state and age. State-specific controls still use `task-controls.js` and `workspace-answers.js`. |
| Board, question and agent routes | `project-blocks.js`: forge/title open the recorded issue; number toggles; runtime and question shortcut open the exact registered board/root/task/revision in Conversations. |
| Native and progress destinations | `tray.rs` supplies separate native/progress URLs and reasons. `project-blocks.destinations` shares their labels across details, task threads and Work sessions; legacy adapter URLs remain exact. Local/remote Claude, Codex, codex-app and HTTP/HTTPS relay targets retain their existing boundaries. |
| Technical facts and history | Runtime, host, role, session, attempt, revision, total age and ordered stages remain disclosed. #1918's canonical append-only Q&A also renders in archived task threads. |
| Closed reading and pins | Existing exact board/transcript reads feed an ordinary reader with issue, PR, outcome, earlier-page and Back controls. First open still removes the pin; the independent reader survives. Completed can reopen it. The 24 h and archive policies remain. |
| Task activity | Existing Work sessions records open the same exact task thread. Role/host/attempt, independent owner and task drafts, and Back context remain. Reading creates no session, sends no input and marks nothing read. |
| Queued Review | `snapshot.rs` exposes the existing queue receipt/result; `display.rs` names queued Review. Inline content shows available CI detail or explicitly unknown progress, without repeating the parent state. Counters/order stay Review. |

## Evidence and checks

Host: macOS 27.0.1, Node.js 22.18.0, Playwright 1.64.1 Chromium headless shell, device scale 1, system UI font. Fixtures are invented. Browser platform flags are data fixtures, not native renderer evidence.

- Before: `node tests/task-navigation.browser.cjs` failed on Doing because `#taskq-popover` existed. The final same boundary checks all eight inline states and forbids floating task panels.
- Before: the activity-to-task route hid Back on Conversations. The new route test exposed it; the shell now retains Back when a history entry exists.
- Visual inspection caught whole-panel Review tint and missing Doing/Failed edge mappings. The shared stylesheet keeps body text neutral and maps the existing semantic classes to their state edge/action accents.
- Test updates relocate popover assertions to the inline/ordinary reader, retaining control payloads, safe native targets, focus, scrolling, history and expiry protection. A clean `33a28abc` baseline also failed the old `stale` expectation: #1918 deliberately exposes `changed`. The updated assertion retains the draft and verifies that refresh/reading sends nothing; explicit Send follows the existing note contract.
- `python3 scripts/check.py --base b0c5269e498824d208ddd04e4da86b3c0d45b45f`: **sandbox-blocked**, 400.102 s. Rule gate passed; 712 unit tests passed, 35 ignored; 20 integration tests passed, one ignored. The unchanged `telemetry_process::resume_records_its_result_without_releasing_the_host_hold` failed with `sandbox-exec: sandbox_apply: Operation not permitted`. No guard was weakened and the full gate was not rerun (R12; §7).
- Isolated `cargo +stable run --locked -- selftest` with `TASKQ_SELFTEST_OFFLINE=1`: **Pass**, 113 golden traces, formats and committed staged telemetry. Board/tool probes remain unknown by design.
- All `tests/*.test.cjs`, `docs/shots/check.test.sh`, `.github/release-tag.test.sh` and `.github/release-check.test.py`: **Pass**. Remaining browser checks were resumed independently after the sandbox-blocked gate.
- Browser selection: **Pass**. It includes closed pins, Conversations selection, project blocks, task dashboard, task navigation, published workspace and task-list wheel/fit checks. The new matrix has 64 cells (8 states × 2 widths × 2 themes × 2 fonts), not 128; existing list checks also cover 14/18 px and coarse-pointer targets.

| Acceptance | Source/model, adapter and browser status |
|---|---|
| A1 | Pass: all eight states, state edge, action parity and one list scroll. |
| A2 | Pass: no repeated structural parent identity; standalone heading and current/historical/archived Q&A retained. |
| A3 | Pass: exact board/agent/question routes; click/Enter/Space only. |
| A4 | Pass targeted Rust session-adapter and JS destination checks; final browser checks passed. |
| A5 | Pass: self-hosted GitLab identity, Close/Escape focus, outside retention and real wheel deltas. |
| A6 | Pass #1829 exact reads, first-open/24 h unpin, unknown/retry and archive checks; independent-reader checks passed. |
| A7 | Pass: exact activity thread, role/host/attempt, no create/send/read acknowledgment, drafts and Back. |
| A8 | Pass UX30-07..10 source/browser checks; canonical gate remains sandbox-blocked, never Pass. |

Native macOS WebKit/Windows WebView2 interaction, OS link opening and screen-reader qualification: **Not tested**, retained for #1932/#1933. No installation, deployment, service, model, effort or permission change was made. Publication still requires green merge-result `merge_group` CI (§7/R12).

## Review set

The complete resumed selection passed, including the separately added #1829 selector update. A test-only screenshot cleanup initially held a stale locator after receipt dismissal; using the live first locator fixed it. Only that failed check and its not-yet-run successors were repeated. The final navigation/workspace/task-list group took 229.231 s. The full canonical gate remains sandbox-blocked.

Three distinct screens were visually inspected at actual size. The review screenshot is scrolled below the sticky filter to show the complete parent and panel; no product styling was changed for capture. Screens qualify these browser fixtures, not the installed app.

| Screen | Observation |
|---|---|
| [Before: Doing](before-doing.png) | Clean `33a28abc` baseline: a floating panel repeats the row title/state/age. This fixture has no injected palette; it establishes structure, not color acceptance. |
| [Inline queued Review](inline-review.png) | Neutral raised content, restrained purple edge/actions, one parent identity, sourced CI detail and complete actions/technical disclosure in the shared scroll. |
| [Closed reader](closed-reader.png) | First-open auto-unpin leaves a stable ordinary read-only page with issue, PR, outcome, transcript and Back. |

Artifact SHA-256 (same application sources throughout the final checks):


- `build/web/task-workspace.js`: `4b70b629ea2d3f96d9f31346ca3867df617717dfc0cb95b3d206e1e1f4329526`.

- `build/web/task-workspace.css`: `ef54d7bd7514bbf4b9f3b2faa7d6e4152c4a00a6590c469879ad476827ef3617`.

- `build/web/workspace.html`: `5c087087c25ccdb8bf9916e354afdc78505120e72a531724bd1e67df03a23661`.
