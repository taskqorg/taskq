# Mac, claude runtime: PM lab run 2026-10-28

Build: TaskQ 4.6.40-dev.1793188598 (source 3ce611c4), the binary pinned in [../report.md](../report.md). macOS 27.0.1 arm64, WKWebView, Retina 2x, dark theme, default font, Conversations window 800x600 points. Project agentq, session claude on mac.

Redaction: a filled box covers the brief file path in the first tool row and other boards' task titles in the sidebar. Nothing else is changed.

| Step | Result | Evidence |
| --- | --- | --- |
| 3 New conversation, first turn | Pass: reply ALPHA | 05 |
| 4 Second turn | Fail in the default flow: Send refused with "open in claude; close it there first. Draft kept." The idle Claude background session started by the first turn holds the conversation. After `claude stop <id>` the resend got BRAVO; the draft was kept | 05 |
| 5 Switch thread and revisit | Pass: history kept | 06 |
| 5 Close and reopen from the menu | Pass: history kept | 07, 08 |
| 5 Wheel through history, Jump to latest | Pass in the recording | recording |
| Thread title | Defect: the title field follows the draft text | 05 |
| First tool row | Observation: the absolute brief path is shown | redacted in 05-07 |

Recording: conv-scroll-send.mp4, 240 s, 800 px wide, sha256 `222d1df2d52ff27223a6b218b895c446275bb8b769912cf6bd913b05ba521c6f`. It is kept on the lab Mac under `<install>/evidence/1743/` and not committed, because it shows the unredacted brief path while scrolling.

Not tested: codex (New request offers only "Claude Code · mac"), step 7 restart (live workers on the host), step 8 refusal while a turn is working, step 9 1x scale and the 350/620/940 widths, Windows WebView2 (no Windows lab run).
