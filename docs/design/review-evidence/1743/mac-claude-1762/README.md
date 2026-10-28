# Mac, claude runtime: PM lab after #1762 (#1753)

Build: TaskQ 4.6.40-dev.1793202220 (source 9d4126b7, carries [#1765](https://github.com/taskqorg/taskq/pull/1765) for [#1762](https://github.com/taskqorg/taskq/issues/1762) and #1769), pinned in [../requalify-1753.md](../requalify-1753.md). macOS 27.0.1 arm64, WKWebView, Retina 2x. Project agentq, claude on mac, session `aaf749f6`. Run 2026-10-28 15:47-15:51Z.

The recording and screens stay on the PM host and are not committed: they show the local brief path. The tables cite them by name and SHA-256.

| Step | Result | Evidence |
| --- | --- | --- |
| 3 New conversation, first turn | Pass: one reply (`ONE`) | 01 |
| 4 Second turn in the same thread | Pass: reply `two`; #1762 no longer refuses the Send | 02 |
| 5 Wheel scroll up and down | Pass | 03, recording |
| 5 Switch thread and back | Pass: history kept, the other thread's draft kept | 05, 06 |
| 5 Close and reopen from the menu | Pass: history kept | 07 |
| 6 Same session id after reopen | Pass: Details shows `aaf749f6` before and after | 04, 08 |
| No task side effect | Pass: `taskq status` shows no new task | PM readback |

Note: reopen lands on the pending #1753 ask, not on the last thread; the thread is one click away.

| File | SHA-256 |
| --- | --- |
| conv-1762.mov | `96550aa0e3907f62d9d78df453158edd9a97584974ea42de86ce44e6fc88ab7b` |
| 01-reply-one.jpg | `69c50188a1b629cc163343d9e5278992860c964cd3eafec43ff58782a6f2435e` |
| 02-reply-two.jpg | `be50e8001e40369aaf2b8a5f7be063023a6e50aa702fc911c825aaa0ac257b2f` |
| 03-scroll-up.jpg | `89eddc051952e3f5d5a14bd7892fbbd286e99a9e096f17273e5964244f4c28c6` |
| 04-details-session.jpg | `5c2392edeeebd2033305233851dd085d0fe466e8308fd5287f848575e2bb2dac` |
| 05-other-thread.jpg | `4986cfea3332cf66678a8d010bbffc2077f8b107409462462a9f3274dc5dfe2e` |
| 06-back.jpg | `1f521c1e3b4e714f8c72e9f46149ab0a42d95265ea2a7522ea0f22f14eb16061` |
| 07-reopen.jpg | `f2073a8ca456db1eca64054345bd48ef4abf062d94520a97922eb9ea56c97ff7` |
| 08-reopen-details.jpg | `d362617127e62d708eb9905fe8d68dcd0f04e2e6d3b2432b7dc7e5b4a514e701` |

Not tested: codex (the composer offered only Claude Code on mac); step 7 restart readback; step 8 one-writer refusal mid-turn; step 9 1x scale and the 350/620/940 widths (Q-A10); Windows WebView2.
