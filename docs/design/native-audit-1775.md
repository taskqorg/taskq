# Native design audit · #1775

[#1775](https://github.com/taskqorg/taskq/issues/1775) audits the installed Mac tray against [review-handoff](review-handoff.md) R01–R44, [ai-workspace](ai-workspace.md#workspace-dialogue-d1d8) D1–D8 and the [visual-style](visual-style.md) task-state and control icons.

## Method

- First pass: the installed Mac dev build (`taskq tray --snapshot`, real WKWebView, real install data: 9 projects, hosts mac, win, hermes-fra1).
- Second pass: the PR head built as a debug binary, same command, same install data. The dev install of the PR head (`scripts/dev-install.sh`) is sandbox-blocked in the worker: it cannot write the install's lock directory. The service's own dev install takes the merged commit.
- Pass means the native screen shows it. A source or browser-fixture pass is not a native Pass.
- Not testable names why: an owner gesture (drag, hover, click sequence, close/reopen, restart), a live effect that would disturb running workers, Windows, DPI or a remote host. Screen recording is not granted to the worker, so only single-route snapshots exist.
- `conversations` snapshots use fixture threads (the route renders dark only); every other route reads the real install.

Evidence: [native-1775-2026-10-29](review-evidence/native-1775-2026-10-29/). Each image puts the native screen (left) beside the approved target render of the same screen (right): Home and Settings from [#1660](evidence/1660/README.md), Hosts, Settings and Services from [#1661](evidence/1661/README.md), Conversations from [#1580](evidence/conversations-1580/rendered/manifest.json). `views-light.png` holds the two native View states. The Agents route shows the account and is not committed.

## First-pass failures and fixes

| Gap | Rows | Fix |
|---|---|---|
| Menu host line read `mac ?` with no counts while a Claude worker ran: a reviewer execution's role read null, so the host fell to unknown | R16 | `bc2c88ce` |
| Strip clipped `hermes-fra1` to `her` | R16, R30 | `bc2c88ce` |
| A host seen only by a stale or unreadable read showed zero counts | R16 | `28c83ad7` |
| Conversations Archive/Restore used non-catalog glyphs | R43 | `30cdd306` |
| Settings widgets, Detach/Return and Work sessions used other icons than visual-style | R28, R43 | `5d9d9cc2` |
| Attention used triangle-alert, Open log, lifecycle and Pause/Resume icons differed from visual-style | R43 | `28c83ad7` |
| Project page Notifications used a `◉` text glyph; Capacity used the Stop square | R43 | `fix(menu)` after this audit commit |
| `recovery` snapshot faulted (`page-error`): its fixture clicked a button that exists only under a reviewed Stop | R35 | this commit (`tests/fixtures/recovery-review.js`) |

## PM steer gaps

The PM steer named gaps on 4.6.40-dev. Each row compares the native screen with the target render in [native-1775-steer-2026-10-29](review-evidence/native-1775-steer-2026-10-29/) (native left, target right). Native screens come from the PR head binary (`taskq tray --snapshot`), not a PR-head dev install.

| Gap | Target | Native result | Fix |
|---|---|---|---|
| M1 Open search field instead of an icon | input-scroll 08 | Pass: Search is an icon beside Controls; the field opens on the icon or a query and stays open while it has text (`01`, `03`) | steer commit |
| M2 No green run button | input-scroll 07, 09 | Pass for layout: the held Start sits inline after the state chips as a green play icon when more than one task is held. This install has Held 1, so the shortcut is absent and per-task Start stays in details | steer commit |
| M3 No project folder row with board icon | input-scroll 09 | Pass: each group row is board icon, project name (overview) and folder icon (`01`, `02`, `04`) | steer commit |
| M4 Host line `mac ?` | input-scroll 07 | Pass: `hermes-fra1 mac ✳1 win` (`01`) | steer commit (R16) |
| M5 Extra `Running` tab | input-scroll 07 | Pass: `Running` is an owner-saved pinned view (R12–R14), not a builtin; builtins are All tasks and Completed. Kept per conversations-preservation-audit | none |
| C1 No Conversation info row | input-scroll 06 | Pass: one collapsed `Conversation info` row heads the reader and shares the reading scroll (UX07); a long thread scrolls to the end, so the row sits above (`06`) | none |
| C2 No info icon beside Send | input-scroll 06, usability 03 | Pass: info icon after Send opens file limits and message help (`05`, `06`) | steer commit |
| C3 No delivery status line | usability 03 | Not testable natively: the line appears after a live send. Text now reads `Message delivered · PM replies here.` | steer commit |
| C4 Raw tool lines with local absolute paths | usability 03 | Pass in source and browser test: tool lines shorten absolute paths to `…/file`; fixture threads carry no tool line | steer commit |
| C5 Open sidebar search field | input-scroll 08 | Pass: sidebar search is an icon; the field opens on click or an active query (`05`–`10`) | steer commit |

Browser tests `tray-state-filter` and `project-blocks` fail identically on the pre-steer source; they are pre-existing and outside this change.

## Review handoff R01–R44

| Row | Design source | Native result (second pass) | Fix |
|---|---|---|---|
| R01 | review-handoff R01 | Pass: Home is host strip, filters, then tasks; no state prose (`home-*`) | |
| R02 | R02 | Not testable: click/Enter opening and no hover opening need pointer and key events in the native window | |
| R03 | R03 | Not testable: the install has no open question with numbered choices; Attention lists 3 items without a parsed card | |
| R04 | R04 | Pass: rows show stage time (`26m`, `5d`, `1h`). Total age and steps in details: Not testable (opens on a click) | |
| R05 | R05 | Pass: Claude/Codex mark directly before stage time; csgo #378 carries the network mark for its win run. Opening a session target is a click: Not testable | |
| R06 | R06, visual-style revision contract | Pass: quiet task dividers, opaque popover surface, light and dark | |
| R07 | R07 | Pass: icons keep one row; Back and `Settings › Services and support` take row 2 (`services-*`) | |
| R08 | R08 | Not testable: one Home activation is a click sequence | |
| R09 | R09 | Pass: one Conversations icon with its count badge (`conversations-badge`) | |
| R10 | R10 | Not testable: needs Settings round trips, close/reopen and restart | |
| R11 | R11, task-counters | Pass: state chips in Doing, Review, Ask, Failed, Ready, Blocked, Later, Held order with a scoped `All tasks 8`; no reorder controls (`home-*`, `views-light`) | |
| R12 | R12 | Not testable: Reset appears only after an edit to a saved view | |
| R13 | R13 | Not testable: deleting a saved view writes owner state | |
| R14 | R14 | Not testable: the install has no pinned task; pinning writes owner state | |
| R15 | R15 | Pass: strip, filters and tasks share the one content scroll | |
| R16 | R16, menu.md | Pass: `hermes-fra1 mac ✳1 win` with no `?` (steer pass). Waiting and ended executions on a fresh read no longer count as unknown. Failed in pass 1 and in the steer review | `bc2c88ce`, `28c83ad7`, steer commit |
| R17 | R17 | Not testable: host lines render collapsed (`hosts-*`); the CPU/memory expansion opens on a click. Board tasks read apart (`Tasks: Doing 2 · Review 1`) | |
| R18 | R18, agent-settings | Pass: tray and service versions apart from runtime client version, account, method, expiry or `Expiry unknown`, check age (Agents route, not committed: shows the account) | |
| R19 | R19 | Not testable: every runtime on this host reads Signed in; Expired, Rejected and Check failed need a failing probe; remote values need the remote host | |
| R20 | R20 | Pass: one Appearance route with text size, tray icon and growth previews | |
| R21 | R21 | Not testable: Auto growth follows the native anchor; needs opening at a screen edge and a drag | |
| R22 | R22 | Not testable: the snapshot renders Settings inside the menu window; the separate window opens on a click | |
| R23 | R23 | Pass: Close always visible. First drag enabling Keep open: Not testable (gesture) | |
| R24 | R24 | Pass: Always on top (overlapping squares, filled front) in the stable row | |
| R25 | R25 | Pass: the menu has no Enlarge; the corner grip has no button chrome. Settings window Enlarge: Not testable (separate window) | |
| R26 | R26 | Pass: footer is `TaskQ v4.6.39` plus grip | |
| R27 | R27 | Not testable: Completed opens with a click and a board history read; no snapshot route | |
| R28 | R28, visual-style control icons | Pass: twelve compact widgets with type, bell, chart-no-axes-column, network, focus, bot, folder, git-pull-request, activity, octagon-x, rotate-ccw, info. Failed in pass 1 (icons) | `5d9d9cc2` |
| R29 | R29 | Pass: Agents route has no circular Access placeholder | |
| R30 | R30 | Pass: Services shows the host and its PID once, then agentq, coreq, dashq by full name (`services-*`) | `bc2c88ce` |
| R31 | R31 | Pass: compact Projects, Delivery (`auto-merge (7) · direct (2)`) and About rows | |
| R32 | R32 | Pass: Work mode widget leads with admission (`Blocked work: all 9 projects (access)`) then intake | |
| R33 | R33 | Pass: Notifications shows event choices apart from OS authorization; the debug binary is not a bundled app and reads `OS unsupported` | |
| R34 | R34 | Pass: Activity shows period, interval, legend with counts and event bins (`cadence` route) | |
| R35 | R35 | Not testable: no reviewed Stop on this install, so no recovery targets; a Stop would halt live workers. The route now renders (`Review recovery…`) | this commit |
| R36 | R36 | Pass: request rows and delivery state in Conversations (fixture threads) | |
| R37 | R37, ai-workspace D1–D8 | Pass: Work sessions sits apart from owner requests (terminal icon); Open session present | `5d9d9cc2` |
| R38 | R38 | Not testable: New, Archive with Undo and Restore are click sequences; the Archive tab and buttons show the catalog icons | `30cdd306` |
| R39 | R39 | Pass: one composer with attach, file, history and send (`conversations-dark`) | |
| R40 | R40, taskq.md § 2 | Not testable: needs a live question and an answer dispatch. The `answers` snapshot route is stale since answers moved to the workspace (#1505): its fixture looks for menu answer cards | |
| R41 | R41, visual-style task states | Pass: see the icon table | |
| R42 | R42 | Pass: Review purple in both themes; merged result not on screen (no merged task in the open list) | |
| R43 | R43, visual-style assets | Pass: every icon on the audited routes is a Lucide 1.17.0 catalog vector; `check-web` rejects unknown names. Failed in pass 1 | `30cdd306`, `5d9d9cc2`, `28c83ad7` |
| R44 | R44 | Pass: this audit; Windows and DPI rows are Not testable on this host | |

## Workspace dialogue D1–D8

| Row | Design source | Native result | Fix |
|---|---|---|---|
| D1 | ai-workspace D1 | Not testable: the `conversations` route renders fixture threads, not a native transcript | |
| D2 | D2 | Not testable: Send starts a native turn on a live session | |
| D3 | D3 | Not testable: needs a session open in the runtime | |
| D4 | D4 | Pass for layout: transcript, then composer, in one thread scroll at 18 and 20 px (fixture). Paging and anchor: Not testable | |
| D5 | D5 | Not testable: turn states need a live turn | |
| D6 | D6 | Not testable: a watch sends turns on a schedule | |
| D7 | D7 | Not testable: watch Details need a launched thread with `watch.json` | |
| D8 | D8 | Not testable: rule files live in the session, not the UI | |

## Task-state icons

| State | Icon | Native result |
|---|---|---|
| Doing | play | Pass (chip and rows) |
| Review | git-pull-request | Pass, purple |
| Ask | message-circle-question | Pass (chip) |
| Failed | circle-x | Pass (chip) |
| Ready | circle | Pass (chip) |
| Blocked | link | Pass (chip, agentq #167) |
| Later | clock | Pass (chip, agentq #168) |
| Held | circle-pause | Pass (chip) |
| Merged | git-merge | Not testable: no merged result on the open list |

## Control icons

| Meaning | Icon | Native result | Fix |
|---|---|---|---|
| Admission | play / pause | Pass (Stop and Work mode routes) | `28c83ad7` |
| Controls / Work mode | sliders-horizontal / focus | Pass (filter button; Work mode widget) | `5d9d9cc2` |
| Attention / question / failure | circle-alert / message-circle-question / circle-x | Pass (strip `3`, chips) | `28c83ad7` |
| Project / pin / add | folder / pin / plus | Pass (group rows, Projects) | |
| Board / issue | github / gitlab | Pass (group and task rows) | |
| Conversations / Work sessions | messages-square / terminal | Pass | `5d9d9cc2` |
| Attach / send / history | paperclip / arrow-up / history | Pass (composer) | |
| Detach / return / close | panels-top-left / panel-left-close / x | Pass for Detach and Close; Return shows only in a detached window: Not testable | `5d9d9cc2` |
| Search / archive / restore | search / archive / archive-restore | Pass | `30cdd306` |
| Runtime | claude / openai | Pass (task rows and strip) | |
| Remote host | network | Pass (csgo #378) | |
| Log / folder / restart | file-text / folder-open / rotate-cw | Pass (Services) | `28c83ad7` |
| Stop / uninstall | square / trash-2 | Pass (Services lifecycle summary) | `28c83ad7` |
| Appearance / cadence / agents / services / about | type / chart-no-axes-column / bot / activity / info | Pass (Settings) | `5d9d9cc2` |
| Notifications / capacity (project page) | bell / sliders-horizontal | Pass (README project shot) | `fix(menu)` after this audit commit |

## Not covered on this host

- Windows tray and per-monitor DPI.
- hermes-fra1 and win sign-in and resources (remote hosts).
- Conversations light theme: the snapshot route renders dark only.
