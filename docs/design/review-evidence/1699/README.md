# TaskQ #1699: Conversations native captures

Native macOS captures (WKWebView snapshot, macOS 27.0.1 arm64) of the Conversations workspace at 800×600, made by [`capture.sh`](capture.sh) through `taskq tray --snapshot --route conversations` on the PR #1708 build. Each step opens the page the way the menu badge does (`open: {page: conversations, thread: unread}`) over the state in [`conversation-shots.js`](../../../../tests/fixtures/conversation-shots.js): ask task #841 with two choices, an unread launched setup request and the owner conversation `Owner plan` with a draft.

| Theme | Size | Badge open | Task question in thread | Owner conversation with draft |
|---|---|---|---|---|
| Dark | 18 px | [badge](popover-dark-conversations-badge-18.png) | [question](popover-dark-conversations-question-18.png) | [owner](popover-dark-conversations-owner-18.png) |
| Dark | 20 px | [badge](popover-dark-conversations-badge-20.png) | [question](popover-dark-conversations-question-20.png) | [owner](popover-dark-conversations-owner-20.png) |

What the captures show:

- Badge open keeps the thread list and selects the unread task question #841 (Needs reply); the setup request stays Unread and nothing is marked read.
- The task question renders in its thread with choices 1 ★ and 2, Checked & context, and the answer composer keeps a typed draft.
- Selecting `Owner plan` shows its draft, project and runtime (Claude Code · fixture-host) while the task row keeps Needs reply.

Limitation: these are WKWebView snapshots of the window surface, not screen captures; Screen Recording is not granted to the worker. Data is invented; snapshot mode refuses `answer`, so nothing reaches a board or a live task (the composer shows Saving... because no receipt comes back). Light theme is left out to keep the set under ten screens.
