# Slice 5 re-qualification after the send and instrument fixes (#1753)

Re-runs the [#1743](report.md) gaps on builds that carry [#1747](https://github.com/taskqorg/taskq/issues/1747), [#1748](https://github.com/taskqorg/taskq/issues/1748) and [#1749](https://github.com/taskqorg/taskq/issues/1749). Same criteria and [PM runbook](report.md#pm-runbook).

## Pins

| Item | Mac, second lab (after #1762) | Mac, first lab | Windows |
|---|---|---|---|
| Source | `9d4126b75ba5fbff2cd4a4662fd9ed540522bd7f` (main, adds #1765, #1769) | `3b5c8208e2e6dece5dd8aab48121cae74694d6fd` (main, #1751, #1756, #1759) | Not pinned: host runs release 4.6.39, which predates the fixes |
| Installed build | `4.6.40-dev.1793202220`, dev channel (install receipt) | `4.6.40-dev.1793193977` | `4.6.39` (heartbeat) |
| Release binary SHA-256 | `c372ca5919c11e24af9a4b894daad23c211031bcaccbef0ea854bcc08e856688` | `8484a9881f2de70e471d53e3d375ec02422d919059245ac2aeb68b723f70aabf` | Not pinned |
| App binary SHA-256 (`TaskQ.app/Contents/MacOS/taskq`) | `0a070109712ad7904f5e27784780d8e24969a76e2251ced00078b256f46f0ab1` | `11afce7b667d499408f66e3043476f19239eacae0f3af22c6db8ce66fffc31be` | Not pinned |
| OS, renderer | macOS 27.0.1 (26A434) arm64, WKWebView | same | WebView2: not pinned |

## Fixture instruments (headless Chromium, macOS)

Instruments, not native runs (R12). Run at 3b5c8208 on the Mac above, 2026-10-28, `node tests/<name>.browser.cjs` after `npm ci` and `npm run build`, twice each.

| Instrument | #1743 | Now |
|---|---|---|
| `tray-task-list` | PASS | PASS, twice |
| `conversations-selection` | PASS | PASS, twice |
| `conversations-stable` | PASS | PASS, twice |
| `conversations-render` | FAIL (`following hides Jump to latest`) | PASS, twice |
| `dialogue` | FAIL (`last chunk in file order`) | PASS, twice |
| `tray-window` | FAIL (`TypeError ... reading 'all'`) | PASS, twice |

Fixture cells Q-A4 and Q-A5 move from Fail to Pass.

## Native lab: Mac claude

The PM ran the [#1743 runbook](report.md#pm-runbook) twice. First on 3b5c8208: [mac-claude-1753](mac-claude-1753/README.md); the second Send was refused ([#1762](https://github.com/taskqorg/taskq/issues/1762)). Then, after #1762 closed, on 9d4126b7: [mac-claude-1762](mac-claude-1762/README.md), with a recording. Owner answer 1 on #1753: Mac lab, Windows Not tested.

| ID | Mac WKWebView (claude), after #1762 | Windows WebView2 |
|---|---|---|
| Q-A12 two PM turns | Pass: replies `ONE` and `two` in one thread (first lab: Fail, #1762) | Not tested: 4.6.39 |
| Q-A4 scrolling | Pass: wheel scroll up and down (recording); Jump to latest not exercised | Not tested: 4.6.39 |
| History on switch and reopen | Pass; the other thread's draft kept | Not tested: 4.6.39 |
| Thread title keeps the first message (#1748) | Pass | Not tested: 4.6.39 |
| Same session id after reopen | Pass: `aaf749f6` in Details before and after | Not tested: 4.6.39 |
| Restart | Not tested | Not tested: 4.6.39 |
| One-writer refusal mid-turn | Not tested | Not tested: 4.6.39 |
| Q-A10 1x, 350/620/940 widths | Not tested | Not tested: 4.6.39 |
| codex runtime | Not tested: composer offers only Claude Code | Not tested: 4.6.39 |

Observation, not a criterion: reopen lands on the pending #1753 ask, not on the last thread.

## Task acceptance

- A1: Pass for claude on Mac at 9d4126b7 (two turns, history revisit, thread switch, reopen); restart and one-writer refusal Not tested; codex Not tested (not offered); Windows Not tested.
- A2: Mac WKWebView ran at 2x only; 1x and other widths Not tested. Windows WebView2 Not tested: the host still runs 4.6.39, which lacks #1747-#1749 and #1765.
- A3: one interaction recording (`conv-1762.mov`, scrolling and both Sends) and eight review screens, kept off the repository because they show a local path, pinned by SHA-256 in [mac-claude-1762](mac-claude-1762/README.md). The defect found on the way is [#1762](https://github.com/taskqorg/taskq/issues/1762), fixed in its own task.
