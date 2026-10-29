# Project group headers · #1805 · stage 1

A compact project band makes the group boundary, counts and four independent targets recognizable. The [owner answer](https://github.com/taskqorg/taskq/issues/1805#issuecomment-6489847031) approves moving Board and Folder together to the right, retaining name → overview and adding a separate collapse arrow. Visual selection remains open. Runtime is unchanged.

Canonical candidate: [menu M2](../../menu.md#m2-scope-and-projects). Editable reference: [mockup](mockup.html), `?variant=a` or `?variant=b`. Open it from a checkout; it uses the existing product stylesheets and bundled icon bytes. The mockup uses invented tasks and local preview-only disclosure storage. Its board, folder and overview buttons show simulated destinations, never perform native actions.

## Usability and preservation

- Existing source, `src/task-dashboard.js::projectHeader`, renders Board → name → Folder. Its caller in `list` supplies only the project, with no counts or disclosure. The heading is secondary text, so it has weak separation from task rows.
- Put recognition first: the neutral band and bold name define a group. Keep direct Board/Folder access together. The name keeps its already approved overview action; moving it to collapse would silently remove that shortcut.
- Keep collapse separate from navigation: a labelled arrow with expanded state changes only row visibility. No nested interactive targets, queue writes or changes to filters. Back returns focus and list context.
- Counts retain both labels when equal. Collapse does not change the filter count. Unknown values retain `?` and their cause. Matching pins remain globally visible and count once; grouping does not duplicate them.
- The production slice would reach M2, the root Product decision under R13, `task-dashboard.js`/CSS, existing M8 disclosure persistence and affected dashboard/view-state checks. It reuses the current project identity, registered board/folder dispatch, task rows and native navigation store. No new configuration, service control or dependency.

## Variants

A is recommended: counts share the name line when space allows, and wrap below it for long names or large text. It uses less height for ordinary names. B always gives counts a second line, making their position predictable at the cost of taller groups. Both use the same fixture, targets, tinted band, type and icons; neither is merely a theme variant.

![A: compact band, inline counts when they fit](variant-a.png)

![B: counts always below the project name](variant-b.png)

## Before and observed checks

Baseline: `89f007c75e2fa33c27b4cd7bbc9b48b3411ce5d9`, bundled unchanged product HTML, Ready filter, 360 × 760 CSS px, dark, 16 px. The browser reproduction observes A2/A4/A5 failures: Board and Folder split around the name, no project matched/total count and no collapse control. This is an observed production failure, not a fixed runtime.

![Before: product group has split actions, no counts and no collapse](before.png)

Commands: `npm run build`; `node docs/design/evidence/1805/check.cjs --evidence --baseline`. Environment: macOS, Node v22.18.0, Chromium headless shell 156.0.8078.4, 1× scale. `tests/browser.cjs` supplies the safe worker launcher. The prototype is a development instrument, outside the CI test suite; screenshots regenerate only with `--evidence`. `--baseline` reproduces the defect against the unchanged source at this checkpoint; omit it after implementation. The full canonical gate is reserved for the rebased implementation result candidate under §7.4; no result is submitted at this stage.

| Check | Result | Scope |
|---|---|---|
| Baseline A2/A4/A5 reproduction | Fail, observed | Exact current product bundle; no fix claimed |
| Proposed geometry | Pass | Each variant: light/dark × 320/360/470 px × 14/16/18/20 px; 48 cells total; no horizontal overflow; 32 px targets |
| Proposed disclosure | Pass | Space/Enter toggle, rows hide, expanded state changes, preview reload restores collapse |
| Proposed navigation | Pass | Three separate simulated destinations; Back restores trigger focus |
| Equal counts | Pass | `2 shown · 2 total` retains both labels |
| Visual inspection | Pass | Actual-size A and B, long name, equal/unknown counts, collapsed groups; A also inspected in light; palette/assets reused |
| Owner visual acceptance | Not tested | Choose A or B before implementation; these images do not approve themselves |
| Native Mac/Windows and screen reader | Not tested | Browser evidence does not qualify installed rendering, native effects or assistive technology |
| Rule-change gate | Pass | `python3 scripts/check.py --rules --base 89f007c75e2fa33c27b4cd7bbc9b48b3411ce5d9`: changed M2 is named by the trusted owner answer |
| Merge-result CI | Not tested | This is the design checkpoint; no result or merge is submitted |

Intentional differences from the pinned `control-panels.html` direction are confined to A2–A6: header band/name emphasis, grouped right actions, explicit shown/total counts and a separate disclosure. Task titles and metadata retain their full-width flow, existing palette, icon family and text scale. Coarse-pointer targets use the existing 44 px policy; native scaling and assistive-technology behavior remain unqualified.

## Next checkpoint

Choose the visual candidate and reconcile A7 with taskq.md §7.8: A7 demands installed Mac/Windows screenshots on this task, whereas the self-fix contract defers live-host checks to the owner batch in the version PR. Keeping A7 requires an explicit §7.8 exception and access to both native hosts. The recommended normal route uses A, updates Product/M2 under R13 before implementation, preserves the existing M8 store, and follows merge-result CI plus the release native batch. No install, restart or host-policy write has occurred.
