# Tasks list: row menu, chip order, layout fit · #1523

Captured by `TASK_LIST_EVIDENCE=docs/design/evidence/1523 node tests/tray-task-list.browser.cjs` (Playwright Chromium on macOS, invented data). Browser renders, not native WKWebView or WebView2 captures.

| Screen | Shows |
|---|---|
| `mac-light-row-menu.png` | Mixed states in chip order (Doing, Review, Ask, Failed, Ready by p0/p1/none, Later); Failed row menu open with Retry and Later; Blocked chip at 0 (icon centered), Later at 13 (icon and count centered as one group) |
| `mac-dark-row-menu.png` | Same in dark |
| `windows-light-row-menu.png` | Same light screen with the Windows platform snapshot (`platform: windows`) |
| `mac-16-fit-panel.png` | Fit height at 16 px with two added panels (Partial note and batch Start): last row and footer visible, no scroll, no gap |
| `mac-20-fit-panel.png` | Same at 20 px |

The same test asserts row order, no per-row action buttons, Escape focus return, chip centering by bounding box and fit for both edge anchors at 16 and 20 px. The Windows screen is the same Chromium render with the Windows platform snapshot; a native WebView2 capture needs a Windows host.
