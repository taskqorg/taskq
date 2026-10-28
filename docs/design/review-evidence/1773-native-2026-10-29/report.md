# #1773 native Conversations evidence

Native WKWebView snapshots (`taskq tray --snapshot --route conversations`, `scripts/macos-runner.sh`) of the branch release build at the commit that adds this file, over an isolated `TASKQ_INSTALL` from `docs/shots/fixture.sh`. Dark, 18 px text. No board or session is touched.

| Screen | Native | Target |
| --- | --- | --- |
| Owner chat: New chat, icon tabs, project line, composer box with icon tools and arrow Send | [owner](owner-dark-18.png) | [usability 03](../conversations-usability-2026-10-28/03-claude-two-turns.png), [input-scroll 07](../conversations-input-scroll-2026-10-28/07-menu-without-empty-control-rows.png), [input-scroll 09](../conversations-input-scroll-2026-10-28/09-project-board-name-folder.png) |
| Task card with state in the task thread | [question](question-dark-18.png) | [pm-journey 02](../pm-journey-2026-10-28/02-worker-progress-in-chat.png) |
| Badge opens the unread task thread | [badge](badge-dark-18.png) | [preservation 05](../conversations-preservation-2026-10-28/05-daily-pm-and-compact-tools.png) |

Not shown here: the installed dev build on the owner's Mac (the worker sandbox refuses writes under the install folder), the PM status line and bubbles over a live transcript (covered by `tests/dialogue.browser.cjs`), light theme, and Windows (Not tested).
