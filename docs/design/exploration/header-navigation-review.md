# Header navigation: exploratory review

2026-10-22. Source: [header-navigation.html](header-navigation.html). SHA256: `7bcc8e11eb29b823f0da201500c00783383895bf31e3ab3ecca4380df606e435`.

Scope: direct Inbox/Conversations/Settings shortcuts versus a consolidated Menu. Both panels share the same illustrative tasks and view state, retain MP11 overlay placement, and leave task-row composition unchanged. Header layout, counters and taxonomy are proposals. No product approval is inferred from exploration.

## Preservation inventory

Inventory sources: `src/tray.html` menu commands and [menu targets](../menu.md), especially M9/M12/M13. This is a destination mapping, not evidence that those native destinations are implemented.

| Existing responsibility | Proposed destination |
|---|---|
| Host scope, work mode, incident state | Host status; Work mode opens the existing workspace destination |
| Attention and task/project browsing | Global state counters, saved views and task filters |
| Project list and registration | Work / Projects; native picker unavailable in prototype |
| Pause/resume and emergency control | Header, named-host review; execution disabled |
| Text size and tray presentation | Personal / Appearance |
| Agent availability and probe context | Environment / Agents |
| Per-project services, resources, logs, folders, restart, stop until login, nested uninstall and incident consent | Environment / Services; named targets and disabled retained actions |
| Activity history and diagnostics | Support / Activity history and Diagnostics |
| Version and verified update check | Support / About; unknown version and unavailable check are explicit |
| Keep menu open | Header Pin; local preview state only |
| Owner notifications and conversations | Direct shortcuts or Communication inside Menu; separate conversation destination |

## Before-presentation evidence

| Rule / scenario | Observation | Result |
|---|---|---|
| Workflow: meaningful simultaneous comparison | Two panels visible together. Top review controls reveal home or sections in both. Direct shortcuts differ from Menu; navigation content includes Communication only on the right | Pass |
| D2/D4/D11: compact header | Host/actions on the first row, six counters on the second; no per-task ellipsis or new icon indent. CPU/memory are in host details | Pass for inspected layouts; metric choice remains open |
| D3/D6 and visual system: width/type | Dark 470 px and light 320 px apps visually inspected. At 320 px / 20 px text both apps measure scrollWidth=clientWidth=318; full-width titles wrap without icon indent | Pass for browser fixtures |
| D7: section recognition | All section rows have consistent Lucide icons and four labeled groups; no action execution disguised as navigation | Pass for inspected navigation pages |
| D8: access cost | Direct Inbox opens in one activation; Menu then Inbox takes two. Back returns through the corresponding origin | Pass |
| F3: working context | TaskQ preset plus Recover search shows only #103; Settings → Appearance → change text to 20 → Settings → tasks retains preset/query. Inbox → #104 → Inbox → tasks also retains them | Pass for exercised local routes; native persistence unqualified |
| D8: state drill-down | Failed counter shows #106 in both panels, clears quick search and uses all projects; all six global counts remain 1 | Pass for illustrative fixture; global-versus-filtered semantics proposed |
| MP11: overlay retained | Both editors open as overlays. Left task-list y=273.390625 both open and closed; right overlay visually inspected | Pass for browser fixture |
| M12 / separate workspace | Conversations and Work mode render in the separate destination section; closing it leaves header actions available | Pass after selector correction; native routing not tested |
| R1/R8: effects and uncertainty | Pause and Emergency stop name Mac and affected sample projects; execution disabled with reason. No service, board, update or registration effects connected | Pass within prototype scope |
| Runtime / syntax | Script syntax check passed; inspected final browser route has no console errors | Pass for inspected route |
| Native acceptance | Monitor/work-area bounds, Windows DPI, real menu scroll/focus, screen-reader traversal, coarse pointer and actual hover travel | Not tested |
| Full destination behavior | Messenger/session management, real Work mode editor, service receipts, live metrics and update metadata | Not tested; destination previews only |

Corrections before presentation: preserved the shared header font after appearance changes; avoided compound secondary-font shrink; made Back names reflect Settings/Menu/Inbox; separated workspace destination selectors from buttons that open them. The last correction was exercised through Conversations → close → host Work mode → close. No broader acceptance claim follows.

The two browser themes and navigation scenes were visually inspected on the frozen source above. Long section pages use normal preview scrolling; native menu height and scroll restoration remain unqualified. Primary comparison controls are above both panels, not in a bottom carousel.

No additional canonical product decision, queue task, runtime implementation, commit, push or deployment is included. MP11 remains the prior placement-only approval. Task total-age visibility remains unresolved.
