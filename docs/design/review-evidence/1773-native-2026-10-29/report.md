# #1773 native Conversations evidence

Native WKWebView snapshots (`taskq tray --snapshot --route conversations`) taken with the installed dev build 4.6.40-dev.1793220453 from 9fa60427 (install receipt f060764e) on the owner's Mac, its binary run in place over an isolated `TASKQ_INSTALL` from `docs/shots/fixture.sh`; each footer shows that version. Dark, 18 px text. The PM chat reads an invented transcript from the fixture; no board or session is touched.

| Screen | Native | Target |
| --- | --- | --- |
| PM chat: You/PM bubbles, PM status line, project line, attach/file/history icons and arrow Send | [chat](chat-dark-18.png) | [usability 03](../conversations-usability-2026-10-28/03-claude-two-turns.png), [preservation 05](../conversations-preservation-2026-10-28/05-daily-pm-and-compact-tools.png) |
| Task card with state in the task thread | [question](question-dark-18.png) | [pm-journey 02](../pm-journey-2026-10-28/02-worker-progress-in-chat.png) |
| Draft chat: New chat, Recent/Archive icon tabs, project line, no empty control rows | [owner](owner-dark-18.png) | [input-scroll 07](../conversations-input-scroll-2026-10-28/07-menu-without-empty-control-rows.png), [input-scroll 09](../conversations-input-scroll-2026-10-28/09-project-board-name-folder.png) |
| Badge opens the unread task thread | [badge](badge-dark-18.png) | [preservation 05](../conversations-preservation-2026-10-28/05-daily-pm-and-compact-tools.png) |

The file icon (New from draft) shows only once a chat was sent, so the draft chat has attach and history alone.
Windows: Not tested.
