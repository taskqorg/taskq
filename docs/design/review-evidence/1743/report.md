# Conversations daily PM use: native qualification (#1743)

Slice 5 of the [Conversations usability plan](../../conversations-usability-review.md#acceptance-and-recurrence-prevention). The owner chose a PM lab run: the PM drives the installed Mac and Windows journeys and records them; the worker pins identity and keeps the Q-A1 to Q-A12 table. Native cells stay Not tested until the PM run in [runbook](#pm-runbook) lands here.

## Pins

| Item | Mac | Windows |
|---|---|---|
| Source | `3ce611c4ee9b64d633d9b66fd2280ca6613f4f6d` (main, slices 2-4) | Not pinned: host unreachable from the worker |
| Installed build | `4.6.40-dev.1793188598` from 3ce611c4, dev channel | Not pinned |
| Release binary SHA-256 | `a90470a61766f06409d599cbd684b1c9cd652dd96f94d95da80ea790e87e1852` | Not pinned |
| App binary SHA-256 (`TaskQ.app/Contents/MacOS/taskq`) | `dc49b891266e30091f416ae47324f55b6c982b0b2a8ad30202b4831ba3ca36fb` | Not pinned |
| OS, renderer | macOS 27.0.1 (26A434) arm64, WKWebView | Windows, WebView2: not pinned |
| Window area, font, scale, theme | per journey, PM run | per journey, PM run |
| Fixture | live disposable owner sessions; no fixture | same |

The app binary hash differs from the release binary because the bundle holds its own signed copy. Both come from the same install receipt.

## Fixture instruments (headless Chromium, macOS)

These are instruments, not native runs (R12). Run at 3ce611c4 on the Mac above, 2026-10-28, `node tests/<name>.browser.cjs` after `npm run build`.

| Instrument | Outcome |
|---|---|
| `tray-task-list` | PASS (#1740 A1-A5: real wheel gestures, 320/360 px, 14-20 px fonts, both themes; #1523 layout fit) |
| `conversations-selection` | PASS (#1723 Recent/Archive selection, one scroll per pane at 18/20 px) |
| `conversations-stable` | PASS (Q-A2 at 350/620/940, Q-A7, Q-A8, Q-A11) |
| `conversations-render` | FAIL, twice: `claude: following hides Jump to latest` (1 !== 0) |
| `dialogue` | FAIL, twice: `last chunk in file order` |
| `tray-window` | FAIL, twice: `TypeError: Cannot read properties of undefined (reading 'all')` |

The #1742 reviewer reported `conversations-render` and `dialogue` PASS on Windows. The macOS failures are filed as held [#1747](https://github.com/taskqorg/taskq/issues/1747). A Chromium fixture with `platform:'windows'` is not a Windows run.

## Criteria

Pass, Fail or Not tested. Fixture: the instruments above. Native: installed app, PM lab run.

| ID | Fixture (Chromium) | Mac WKWebView | Windows WebView2 |
|---|---|---|---|
| Q-A1 menu wheel over question preview | Pass | Not tested | Not tested |
| Q-A2 fixed heading and Send | Pass | Not tested | Not tested |
| Q-A3 anchor on prepend, append, font, resize | Pass (`conversations-render` cases before its failing step) | Not tested | Not tested |
| Q-A4 reader not moved; Jump to latest | Fail (`conversations-render`: Jump to latest stays after reaching the end) | Not tested | Not tested |
| Q-A5 long sources traversable, bounded nodes | Fail (`dialogue` file order; `conversations-render` traversal and bound passed) | Not tested | Not tested |
| Q-A6 feedback and frame timing | Not tested (no measured run here) | Not tested | Not tested |
| Q-A7 receipts stay on their question | Pass | Not tested | Not tested |
| Q-A8 drafts, refusal, unknown result | Pass | Not tested | Not tested |
| Q-A9 keyboard, screen reader, copy | Not tested | Not tested | Not tested |
| Q-A10 sizes, fonts, themes, 1x/2x | Pass for menu and workspace widths; 2x not covered | Not tested | Not tested |
| Q-A11 failed and late reads | Pass | Not tested | Not tested |
| Q-A12 two PM turns per runtime | Not applicable | Not tested | Not tested |
| Restart | Not applicable | Not tested | Not tested |
| One-writer refusal | Pass (Q-A8 owner refusal) | Not tested | Not tested |

## PM runbook

One journey per host and runtime (`claude`, `codex`). Save files under `docs/design/review-evidence/1743/<host>-<runtime>/`. At most ten screens across the whole package; the recordings carry continuity.

1. Pin: on Mac `shasum -a 256` of the release binary and `TaskQ.app/Contents/MacOS/taskq`, `sw_vers`; on Windows `Get-FileHash` of the installed `taskq.exe` and the WebView2 runtime version. Note the source SHA from the install log.
2. Note window size (points), scale (1x or 2x), font size and theme before you start.
3. Start screen recording. In Conversations, create a new owner conversation on the taskq project, labelled disposable. Ask for a read-only answer only, for example `Reply with the word one. Do not run commands.`
4. Wait for the native reply. Send a second message (`Reply with the word two.`) and wait for that reply.
5. Wheel-scroll up through older history and back down; use Jump to latest if shown. Switch to another thread and back. Close the window and reopen it from the menu.
6. Check: both replies show in one thread; the session id under Details is the same before and after reopen; `taskq status` shows no new task, attempt or comment.
7. Restart: `taskq service restart` with no live workers, reopen the thread, check it reads the same history. Record separately.
8. One-writer refusal: send while the agent is still working on a turn; record the refusal text and that the draft stays.
9. Repeat at 1x and 2x if the host has both displays; resize the window through the Q-A10 workspace widths 350/620/940.
10. Stop the recording. Answer #1743 with the folder names and any Fail with its step.
