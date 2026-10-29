# State-first menu task rows

Task [#1881](https://github.com/taskqorg/taskq/issues/1881) applies M3.9 under R13. The existing state glyph and ask shortcut move before the board mark and issue number. Runtime/session controls, elapsed text, full titles and identity details retain their existing behavior.

## Before and after

Base: `b3e6b733c3cdf9ed1bd35c9e8c583026b44bca96`. Implementation: `069f7dd0` (the later evidence commit retains these UI sources). Both bundles use the same invented fixture and 360 × 720 px browser viewport at 16 px text.

Before changing the component, `node tests/tray-task-list.browser.cjs` failed at the new assertion: `state icon is the first row-head element at the left edge` (actual false, expected true). After the change, the same check passed, including the existing exact question target, pointer target, wheel scrolling, details controls and context preservation checks.

Four screens, captured after the one pre-push rebase:

| Theme | Before | After |
|---|---|---|
| Light | [Compact rows](before-light-compact.png) | [Compact rows](after-light-compact.png) |
| Dark | [Compact rows](before-dark-compact.png) | [Compact rows](after-dark-compact.png) |

The first element changes from `task-identity` to `task-stage` on every fixture row. The ask glyph still sends the exact root, board, task and revision. [Observed order and target checks](observations.json).

## Verification

- `node tests/tray-task-list.browser.cjs`: pass; state-first row heads, macOS/Windows snapshots, both themes, 16/20 px and top/bottom anchors; existing question interaction checks cover 320/360 px and 14/16/18/20 px.
- `node tests/task-dashboard.browser.cjs`: pass; combined/global order remains state, board, remote, runtime, elapsed; existing details, pins, views and history protections remain.
- `node tests/tray-panels.browser.cjs`: pass; Home and project blocks start with state; runtime and exact question navigation remain.
- `node tests/project-blocks.test.cjs`: pass; shared links, sessions, assets and generated provenance remain protected.
- `node docs/design/evidence/1881/capture.cjs`: pass; the four screenshots and exact ask target, using the saved base HTML through `TASKQ_BEFORE_HTML` and the candidate `build/web/tray.html`.

These are browser/fixture observations. Native rendering and owner visual acceptance are not established by them (R12). The selected candidate gate and publication status are reported in the PR and task result (§ 7); merge acceptance requires green `merge_group` CI.
