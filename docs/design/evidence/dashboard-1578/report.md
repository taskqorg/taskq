# Dashboard implementation acceptance (#1578)

R1/R6/R8/R12/R13 and taskq.md §7 apply. This report separates supported UI behavior, board-stage data, native geometry and unqualified platform effects. No deployment, service restart, grant, model or admission change is part of this candidate.

## Source and reference

- Baseline: main `21742cea80e5ac90c0e9e375169fecaae8e09773`; the primary dirty design checkout was preserved. Implementation uses the isolated `taskq-1578` branch.
- Frozen #1573 archive SHA256: `d9f39f3f24bbb4e43d94c113dbdd222bf839ee78e67e86de4fa0b12e2ef1f640`; all 168 TaskQ and 30 shared-file hashes verified. [Original manifest](../frozen-1573-package.json).
- Editable reference: [control-panels.html](../../exploration/control-panels.html), marker `conversation-walkthrough`, SHA256 `9c250e850bbda81814f2a66a654d54b44e617615f2fa2c61fb527b1dd6863a04`. It was not regenerated.
- O1/O2/O3: [owner decisions](https://github.com/taskqorg/taskq/issues/1573#issuecomment-6340259438), reconciled in MP11–MP13, M3/M13, counters, controls and Product. Current Held/no Closed, Preact/Signals/Wouter, owner sessions and #1565 session adapters take precedence over archived snapshots.
- Source/artifact/image hashes and final commands are recorded in `candidate.json`. Shared Lucide static 1.17.0/provider assets retain their licenses; no new dependency was added.

## Preservation and navigation ledger

| Requirement | Destination and supported behavior | Evidence boundary |
|---|---|---|
| M1–M3/MP12 | Tasks dashboard: compact host/running/attention summary, complete task list; direct Inbox and Conversations | Source/browser; native captures |
| MP11/MP13/O2/O3/K | Anchored editor, OR states/projects, search, combined/grouped shared comparator, scoped All; saved/pinned views and independent working criteria/scroll; Save/Reset/restart | Model, actual bundle, native preference persistence |
| O1/M3/task-controls | Identity preview/pinned details, full wrapped issue title; current-stage elapsed; total age, completed count and ordered board-stage durations; all offered revision-fenced actions and same-ID delivery checks | Board read-model and browser; fold/dispatch unchanged |
| M13/U-A2–U-A4 | Reachable Close; thresholded drag pins; retained mode/position, actual-anchor Auto, explicit Down/Up/Fixed; reduced-motion miniatures, fixed footer and keyboard/pointer corner resize | Model/browser; native WKWebView geometry; click/raise owner-observed |
| MP1/CI1/CI4 | Appearance/Tray icon: native-rendered 22px Icon/Summary/saved Custom previews; same renderer on both OSes; font independent of icon calibration | Existing renderer pixel/validation checks; native Mac |
| MP2/MP10/G3 | Settings → Work mode → existing resource review, explicit targets, unchanged/mixed fields, saved/applied receipts; scope/admission review preserves pause/resume/label recovery and original request on Projects | Existing resource/admission model/browser checks |
| P1–P9/B1–B5 | Settings → Agents and Access opens existing workspace editor; Delivery displays project preset/revision and Board | Existing capability/grant boundaries; no new policy write |
| S/M10 | Settings → Projects → existing picker/setup review, all four folder cases and exact external-effects confirmation | Existing setup source checks; no actual registration |
| M9–M12/E1–E5 | Services/support and workspace reviews retain log/folder/resources, restart/Drain/Full stop/uninstall, incidents/consent, selected recovery, update/readback/rollback | Existing action, receipt, browser and Rust checks; no live effects |
| M5/M6/M14 | Activity retains four series/period/timezone/custom range/unknown; Notifications retains host and project modes plus OS status | Existing model/browser checks; no delivered notification claimed |
| C/WS1–7/10/11 | Inbox and Conversations reuse current request/message/owner-session records; work sessions stay distinct; all existing operations/headless routes remain | Communication/operation browser checks; full transcript journey remains #1580 |

Unsupported backend capabilities remain explicit: general future-match/snapshot policy editing beyond current admission/resource adapters, transactional multi-repository execution and expanded remote gateway are not advertised. Custom composition remains the existing validated `taskq tray --icon FILE` workflow; unsaved Custom shows its Summary starting preview and a clear import instruction. No AI icon-generation or new grant channel is claimed. Session adapters remain #1565.

## Fixtures and before/after findings

Fixtures are invented and token-free. `tests/task-dashboard.browser.cjs` covers normal/duplicate boards, canonical repository ties, empty/partial, missing times, long unbroken titles, 320px/short windows, both themes, 16/20px, keyboard/light-dismiss and reduced motion. Existing shell checks retain fine/coarse targets and 200% text. Native fixtures use `docs/shots/fixture.sh` with explicit invented project identities.

| Finding | Before | Fix and retained protection | After |
|---|---|---|---|
| MP11 missing editor | Baseline actual bundle: `[data-a=view-editor]` count 0; no saved views | Shared local view model; filters never write admission | Actual bundle editor, scopes, Save/Reset/switch/restart pass |
| O1 row actions | Baseline right ellipsis and inline action expansion | Identity-anchored dialog; original control table/dispatcher and receipt IDs retained | Details/actions, Escape focus and exact Start retry pass |
| O3 Back | Filtered dashboard initially hid Back after returning from a project | Back uses recorded shell history; per-view context stays local | Navigation/restart/scroll tests pass |
| O1 Escape | Focus restoration reopened the transient preview | Suppress only that restoration's preview event | Escape closes and returns focus without reopening |
| Profiling capacity | Eagerly loaded unused editors exceeded CoreQ's 256-method limit | Load existing answer/resource editors on use; collection limits/inventory unchanged | Menu and operational route checks pass |
| M13 growth | Baseline fixed/top/bottom cycle; no actual-anchor Auto/drag-to-Pin | Preserve old explicit values; actual native placement computes Auto; drag threshold then native Pin | Geometry/model/persistence and browser controls pass |

[Before](browser/before-dark-16.png) and [after](browser/after-dark-16.png) use the same invented two-task fixture. Browser matrix measurements are in [browser.json](browser/browser.json). Native captures and image hashes are indexed by `candidate.json`.

## Verification and limits

Final command outcomes and exact artifact identity are recorded in `candidate.json`; no pending item below is a pass.

- Design accepted: O1–O3 and the frozen package; no new product choice introduced.
- Model/source/browser: selected canonical gate plus affected instruments, including retained controls, operations, navigation and adverse-state coverage.
- Native: isolated signed macOS WKWebView snapshots; usable-area/edge math, growth persistence and resize API remain separate evidence classes.
- Not tested: native Windows WebView2/DPI/multi-monitor interaction, physical drag/Snap, screen reader/IME and live operational effects. Native click/raise remains owner-observed under U-A8/current UI acceptance; it is not a worker gate.
- Not deployed. Release qualification and owner rollout remain separate under §7/§8. Review the exact candidate; browser/static pictures alone do not qualify native Windows behavior.
