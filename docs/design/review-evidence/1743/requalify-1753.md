# Slice 5 re-qualification after the send and instrument fixes (#1753)

Re-runs the [#1743](report.md) gaps on builds that carry [#1747](https://github.com/taskqorg/taskq/issues/1747), [#1748](https://github.com/taskqorg/taskq/issues/1748) and [#1749](https://github.com/taskqorg/taskq/issues/1749). Same criteria and [PM runbook](report.md#pm-runbook).

## Pins

| Item | Mac | Windows |
|---|---|---|
| Source | `3b5c8208e2e6dece5dd8aab48121cae74694d6fd` (main, #1751, #1756, #1759) | Not pinned: host runs release 4.6.39, which predates the fixes |
| Installed build | `4.6.40-dev.1793193977` from 3b5c8208, dev channel (install log) | `4.6.39` (heartbeat) |
| Release binary SHA-256 | `8484a9881f2de70e471d53e3d375ec02422d919059245ac2aeb68b723f70aabf` | Not pinned |
| App binary SHA-256 (`TaskQ.app/Contents/MacOS/taskq`) | `11afce7b667d499408f66e3043476f19239eacae0f3af22c6db8ce66fffc31be` | Not pinned |
| OS, renderer | macOS 27.0.1 (26A434) arm64, WKWebView | WebView2: not pinned |

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

Fixture cells Q-A4 and Q-A5 move from Fail to Pass. Native cells wait for the PM lab run.
