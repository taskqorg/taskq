# Mac, claude runtime: PM lab re-run 2026-10-28 (#1753)

Build: TaskQ 4.6.40-dev.1793193977 (source 3b5c8208), the binary pinned in [../requalify-1753.md](../requalify-1753.md). macOS 27.0.1 arm64, WKWebView, Retina 2x, Conversations window 800x600 points. Project agentq, session claude on mac.

Screens stay on the PM host and are not committed: they show the local brief path. The table cites them by name and SHA-256 prefix.

| Step | Result | Evidence |
| --- | --- | --- |
| 3 New conversation, first turn | Pass: one reply | 01 |
| 4 Second turn | Fail, twice: Send refused with "Not sent: open in claude; close it there first. Draft kept." Filed as [#1762](https://github.com/taskqorg/taskq/issues/1762) | 02 |
| 5 Scroll, switch thread and back | Pass: history and draft kept | 03 |
| 5 Close and reopen from the tray | Pass: history and draft kept | 04 |
| Thread title | Pass: the title stays the first message ([#1748](https://github.com/taskqorg/taskq/issues/1748)) | 03 |

PM cause note for #1762: `holds()` treats state `working` as another writer; `claude agents --json` shows the idle launched job as status `idle`, state `working`.

| Screen | SHA-256 prefix |
| --- | --- |
| 01-one-reply | `9f7833feb96a` |
| 02-second-send-refused | `4a3aef56099d` |
| 03-switch-back-title-kept | `fa7d0fcbaa4b` |
| 04-reopen-history-draft-kept | `3f428e414d2a` |

Recording: none; the capture was lost on stop, so there is no SHA-256.

Not tested: codex (the composer offered only Claude Code on mac); step 6 same session id (no second reply); step 7 restart (live workers on the host); step 8 one-writer refusal mid-turn (every second Send was refused); step 9 1x scale and the 350/620/940 widths; Windows WebView2 (host runs 4.6.39, without the fixes).
