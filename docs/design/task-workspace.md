# Task workspace component

Accepted under [taskq.md § 4](../../taskq.md#4-service) (#1645). TaskQ owns the task workspace: the menu, task list with saved views and the task detail popover. The tray and DashQ Tasks ([dashq#186](https://github.com/taskqorg/dashq/issues/186)) load the same released build unchanged; DashQ keeps no fork. [Menu](menu.md) M1–M14, [task controls](task-controls.md) and the [answer card](answer-card.md) stay the behavioral authority.

## Contract

Version `taskq.task-workspace/1`; the release tag gives the build version.

- Bundle `task-workspace.js` defines the global `TaskqTaskWorkspace` `{schema, createState, create, answerCard, projectBlocks, resources}`. Stylesheet `task-workspace.css` styles a `<div class="menu">`.
- Mount: `TaskqTaskWorkspace.create(TaskqTaskWorkspace.createState(), ports).mount()`. Optional `ports` replace `window`, `document`, `menu`, `send`, `render`, `timers`, `workspace` and `measureScrollbar`; omitted ports use the page.
- Data in: `taskq.receive(state)` with the tray state the Rust service builds: `platform`, `font`, `dark`, `host`, `now`, `projects` (status.json projections), `display` (display model), `service`, `ai`, `notify`, `stop_modes`, `messages`. Each call replaces the previous state.
- Actions out: one JSON object per action through `ports.send`, else `window.ipc.postMessage(JSON)`. Field `a` names it, for example `answer` (`root`, `board`, `op`: a task control or answer), `open` (a link), `set` (view state), `size` (window size). The host maps each action to its own TaskQ dispatcher and shows the receipt; the component holds no credentials, network client or task truth.

## Release assets

Each release ships, under `SHA256SUMS`: `taskq-<v>-task-workspace.js`, `-task-workspace.css`, `-task-workspace-fixtures.json` and `-task-workspace-sources.json`. The sources manifest ([task-workspace.sources.json](task-workspace.sources.json)) lists every bundled source, stylesheet and the fixture; the build derives their SHA-256 (UTF-8, LF) into the released `-sources.json` ([visual evidence](../testing.md#visual-evidence)). The tray embeds `build/web/task-workspace.js` and `.css` byte for byte (`tests/task-workspace.test.cjs`). Consumers pin the version and the `SHA256SUMS` hashes, as for the PM composer and task controls.

## Fixtures

[tests/fixtures/task-workspace.json](../../tests/fixtures/task-workspace.json) holds one state and three journeys, replayed on the bundle alone by `tests/task-workspace.browser.cjs`:

- `narrow` (320×640): rows fit; the view editor opens, a state facet filters, Escape returns focus.
- `large-text` (360×760, font 20): rows fit; a row opens its detail, which fits; Stop sends `answer`.
- `keyboard`: Tab reaches a row without opening its detail; Enter or Space opens the detail with focus in it; Escape returns focus to the row (MP13). The shipped fixture still expects a focus preview until #1659 updates it.

Each step names a selector or key and an expectation (`count`, `open`, `focus`, `text`, `fits`, `sent`), so a consumer replays them on its pinned version.
