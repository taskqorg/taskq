# Task workspace component

Accepted under [taskq.md § 4](../../taskq.md#4-service) (#1645). TaskQ owns the task workspace: the menu and task list with saved views, opening exact tasks in the unified Conversations reader (menu M3.6–M3.17). The tray and DashQ Tasks ([dashq#186](https://github.com/taskqorg/dashq/issues/186)) load the same released build unchanged; DashQ keeps no fork. [Menu](menu.md) M1–M14, [task controls](task-controls.md) and the [answer card](answer-card.md) stay the behavioral authority.

## Contract

Version `taskq.task-workspace/1`; the release tag gives the build version.

- Bundle `task-workspace.js` defines the global `TaskqTaskWorkspace` `{schema, createState, create, answerCard, projectBlocks, resources}`. Stylesheet `task-workspace.css` styles a `<div class="menu">`.
- Mount: `TaskqTaskWorkspace.create(TaskqTaskWorkspace.createState(), ports).mount()`. Optional `ports` replace `window`, `document`, `menu`, `send`, `render`, `timers`, `workspace` and `measureScrollbar`; omitted ports use the page.
- Data in: `taskq.receive(state)` with the tray state the Rust service builds: `platform`, `font`, `dark`, `host`, `now`, `projects` (status.json projections), `display` (display model), `service`, `ai`, `notify`, `stop_modes`, `messages`. Each call replaces the previous state.
- Actions out: one JSON object per action through `ports.send`, else `window.ipc.postMessage(JSON)`. Field `a` names it, for example `answer` (`root`, `board`, `op`: a task control or answer), `open` (a link), `set` (view state), `size` (window size). The host maps each action to its own TaskQ dispatcher and shows the receipt; the component holds no credentials, network client or task truth.

Task lists, Conversations and task detail headers use the same identifier/copy widget, displaying `project#NNN` without a space, including within project groups. Display labels never replace canonical board/task identities or destinations. Its Copy icon appears to the right on hover or keyboard focus and remains reachable on touch. Copy writes the exact displayed text without opening a task or invoking a row action, and reports concise success or failure. Status-bar versions use this same widget and copy the actual displayed version. Every other UI reference to a task (tooltips, accessible names, receipts, reports, notifications and the native Quit label) uses the same `project#NNN` text without a copy button; `Board` stands in only when no project name is known. Search matches the displayed identifier. Long project names in the native Quit label collapse to a `+N` count within its width limit.

## Release assets

Each release ships, under `SHA256SUMS`: `taskq-<v>-task-workspace.js`, `-task-workspace.css`, `-task-workspace-fixtures.json` and `-task-workspace-sources.json`. The sources manifest ([task-workspace.sources.json](task-workspace.sources.json)) lists every bundled source, stylesheet and the fixture; the build derives their SHA-256 (UTF-8, LF) into the released `-sources.json` ([visual evidence](../testing.md#visual-evidence)). The tray embeds `build/web/task-workspace.js` and `.css` byte for byte (`tests/task-workspace.test.cjs`). Consumers pin the version and the `SHA256SUMS` hashes, as for the PM composer and task controls.

## Fixtures

[tests/fixtures/task-workspace.json](../../tests/fixtures/task-workspace.json) holds one explicit live/complete synthetic project state and three journeys, replayed on the standalone bundle and real reader adapter by `tests/task-workspace.browser.cjs`:

- `narrow` (320×640): rows fit; the view editor opens, a state facet filters, Escape returns focus.
- `large-text` (360×760, font 20): all seven rows fit; a row sends its exact Conversations destination once. The host adapter opens that destination in the real workspace bundle; task identity/controls fit, and keyboard Stop sends the revision-fenced `answer`.
- `keyboard`: Tab reaches the accessible whole-row target without navigation; Enter and Space each emit one exact Conversations destination, with focus retained in the originating row and no competing inline reader (M3.9/M3.14).

These are distinct layout/filter-focus, detail/control adapter and keyboard-navigation risks, not a Cartesian size/theme matrix. The narrow journey retains its seven-row and filtered single-row counts; Escape still returns view-editor focus. Native cross-window focus behavior remains a separate host qualification.

Each step names a selector, key or explicit host-reader handoff and an expectation (`count`, `open`, `focus`, `text`, `fits`, `sent`, exact `destination`/`navigationCount`, or revision-fenced `operation`), so a consumer replays them on its pinned version.
