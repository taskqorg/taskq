# Single-source-of-truth review (2026-10-22)

Interactive review with the owner. Base: `ccd33b88` (#1587). Research only; tasks are filed by the PM session after owner approval.

## 1. Icon divergence

Owner sees three different history graphs on the Mac: menu-bar icon, app (Dock) icon, icon inside the menu.

### Where the graph is drawn

| Surface | Code | Renderer | Size it renders at | Bars (notch Mac / non-notch) |
|---|---|---|---|---|
| Menu-bar icon | `src/tray.rs` `refresh_icon` -> `draw` -> `icon::render` | Rust raster | 22 pt x `menu_bar_scale()` x backing scale: 66 px / 44 px | 6 / 4 |
| App (Dock, Cmd-Tab) icon, running | `src/tray.rs` `refresh_icon` -> `refresh_app_icon`, `draw(.., 1.0, 256/22)` | same Rust raster | 256 px, OS downscales | 12 (8 px bars, 1 px gaps) |
| App icon, not running (Finder, bundle) | `assets/TaskQ.icns` / `.ico` from `icon::compact` (test `app_icon_assets`) | Rust, compact q only | 16..1024 | no graph |
| Appearance picker inside the menu (`Icon` / `Summary` buttons) | `src/tray.rs` `state()` `st["icons"]`, `draw(.., 1.0, 2.0)`; `src/tray-app.js` `views()`, CSS `.views img{height:18px}` | same Rust raster | 44 px, shown at 18 CSS px (x0.82, resampled) | 4 |
| Menu History panel | `src/tray-app.js` `history()` + `historyRaster`, CSS in `src/tray.html` | separate JS/DOM chart | menu width, 64 px high | 12 |

### What differs and why

1. **Bar count depends on pixel width by design.** R-UI5 picks the largest B in 12/6/4/3/2/1 with bars >= 4 physical px and 1 physical px gaps. Same data, so 6, 12, 4 and 12 bars on one Mac. Each surface is "correct" to R-UI5; R-UI5 itself makes the icon a different picture at every size.
2. **Gap is 1 physical px at every size.** At 256 px (Dock) the gap is 0.4 % of the icon and vanishes when the OS scales it down: the graph reads as one solid block. At 44 px it is clearly separated bars.
3. **The in-menu picker is resampled.** A 44 px raster is shown at 18 CSS px (36 device px): non-integer downscale blurs the 1 px gaps and counters.
4. **Three color sources.** Icon bars use `icon::palette()`; icon counters use colors hardcoded in `icon::render`; the menu uses CSS tokens in `src/tray.html`. Values differ, e.g. dark "closed" bar `#77cdb3` (palette) vs `#8dd2b8` (counter and CSS); blocked/waiting `muted #b4bdc8` vs `--ready #c8d0d9`.
5. **Two rebin implementations.** `icon::history_rows` (Rust) and `historyRaster` (JS, with an extra `bins.length % n` rule). The menu panel is always wider than 59 px, so in practice it always shows 12 bars; the JS rebin code serves no real case.
6. **Unknown history.** The menu panel marks bins before `since` as unknown; `icon::Data` has no `since`, so the icon draws unknown time as zero (M6: unknown is never drawn as zero).

#1586 reached all Rust surfaces (one renderer) and the JS panel; it did not cause the divergence. The divergence comes from R-UI5's width-dependent bar count plus per-surface sizes and three palettes.

### Options considered

- A: keep the Rust raster, define one geometry in points of the 22 pt reference, one palette.
- B: keep the R-UI5 per-pixel rule, only fix Dock and picker by downscaling the menu-bar raster.
- C: one SVG renderer for every surface.

### Owner decision

- **C: one SVG renderer.** Rust builds one SVG; menu bar and Dock rasterize it, the menu embeds the same SVG.
- **Fixed 6 bins everywhere**, menu History panel included, for every period (bin = period / 6). Bar count never depends on pixel width; R-UI5's 12/6/4/3/2/1 rule and the JS rebin go away.
- **Encoding: plain height columns.** The owner first picked the heat strip (variant 3 in [img/icon-variants.png](img/icon-variants.png)), then replaced it in the Codex session: amount must read by height. Six separated green columns; height = closed count, linear from a common zero to the window peak. Failed is a separate red cap (presence only). Ask and blocked have no mark; exact counts stay in History detail.
- **Palette A** (semantic tokens, shared by graph, icon counters and menu CSS).
- **Custom icon removed:** `taskq tray --icon FILE` and the custom schema go; Icon and fixed Summary stay. A saved Custom choice falls back to Summary.
- Full decisions, geometry and real-data evidence: [visual-decisions.md](visual-decisions.md). Approved screens: [dark](img/surface-review-05-dark.svg.png), [light](img/surface-review-05-light.svg.png).

### Task 1: one icon renderer

- **Goal:** the menu-bar Summary, the running Dock icon, the in-menu picker and the menu History panel show the same six-bin closed-height graph from one SVG renderer and one palette; the Custom icon is gone.
- **Acceptance:**
  1. Spec first: `taskq.md`, R-UI4/R-UI5 (`docs/design/rendering-contract.md`), `icon-summary.md` I1-I6, `menu.md` M1/M6 and `custom-icon.md` say what [visual-decisions.md](visual-decisions.md) approved: 6 source bins (period / 6, whole epoch-aligned bins for presets, exact bounds for custom ranges), plain columns, palette A, the 22 pt reference geometry, and the zero / unknown / ongoing / failed marks. Heat intensity, stacked heights, 12 source bins and width-dependent rebin are removed from the spec.
  2. One Rust function returns the SVG for given data and theme. Menu bar and Dock rasterize it once at their target scale; picker and History embed the same vector groups (History may crop the viewport; its 6 bins stay selectable). `historyRaster`, `icon::history_rows`, per-surface geometry and duplicate color tables are deleted.
  3. Column height = 9 pt x closed / peak; peak = max closed over bins with complete data. Known zero is a neutral baseline tick; unknown or incomplete data is a neutral mid dash and never enters the peak; peak 0 is safe; the bin containing now has a neutral underline; failed > 0 adds a fixed red cap. Failed, ask and blocked never change column height.
  4. Palette A for graph, counters and menu CSS tokens: work/closed `#087F5B` / `#38E8A0`, ask `#995200` / `#FFC247`, failed `#C92641` / `#FF6378`, neutral `#536171` / `#B9C6D8` (light / dark).
  5. History: clicking or keyboard-selecting a bin shows, in the same panel, its `[start,end)` interval with timezone and its exact closed, failed, ask and blocked counts, marked ongoing or incomplete where true. Selection survives refresh and changes no scope, period or admission state.
  6. `taskq tray --icon FILE`, the Custom schema, its validation, rendering and editor hooks are deleted; the obsolete flag gives a clear error; a saved Custom choice resolves to Summary and other settings stay. Counter meanings, compact Q, pause/hold identity and overflow behavior do not change.
  7. Tests: the frozen fixtures `docs/reviews/real-data-2026-10-22.json` and `real-data-hour-2026-10-22.json` give the same bin ids, counts, colors, heights and marks on every surface; three labelled synthetic cases cover peak 0, failed > 0 and unknown input.
  8. Owner approves a review set of at most 10 native screens (light/dark: non-notch and notch menu bar, Dock, picker, History) with real fonts and scale.
- **Scope:** `src/icon.rs`, `src/tray.rs`, `src/tray-app.js`, `src/tray.html`, CLI/settings callers of `--icon` and Custom, `Cargo.toml` (SVG rasterizer; drop dependencies only Custom used), tests; `taskq.md`, `docs/design/rendering-contract.md`, `icon-summary.md`, `menu.md`, `custom-icon.md`, `custom-icon.test.cjs`. Depends on #1578 (it edits `src/tray-app.js`). No queue, worker, service or install change.

## 2. Other single-source breaks

Ranked by what the owner sees.

1. **Freshness: four thresholds.** `src/tray-app.js` marks runs, hosts and tasks unknown after 60 s (`:385-432`); `src/tray.rs:463-468` uses file mtime with 300 s; the CLI uses ttl 300 s (`src/main.rs:1919`, `src/snapshot.rs:30-35`); `src/messages.rs:82` uses 60 s; `src/main.rs:1915`, `:2207` gate `status.json` at 60 s. The heartbeat is rewritten only once it is 60 s old (`src/service.rs:1425`) and the snapshot is read before that write (`:1196` vs `:1421`), so published `observed` is already 60-120 s old. Result: the tray shows false "Run state unknown", "Tasks unknown", "Reservations: unknown" for most of each cycle while the CLI says the same data is live. Same class as #1588 (different cause).
2. **Counts, dedupe and paused/run/watch computed three times**: `src/tray.rs:541,557`, `src/project-blocks.js:10`, `src/tray-app.js:220-222`, `src/main.rs:1896`. They differ on trailing slash and conflict handling; the CLI has no Paused line although `menu.md:249` requires it.
3. **State names, task order, age format** each have their own copy: `tray-app.js:713`, `project-blocks.js:75`, tooltip, CLI; order in `tray-app.js:518`, `:229` (its priority parse misses `priority-2`) and `snapshot.rs`; age in `main.rs:626`, `tray-app.js:238`, `project-blocks.js:43` (13800 s is "3h" in the CLI and "4 h" in the tray).
4. **Small vocabulary splits:** history series names (`tray.rs:534`, `icon.rs:17`, `tray-app.js:271`, legend says "waiting"), notification labels (`notify.rs:9-11` vs `tray-app.js:765`, "Question" vs "Questions"), history periods (`tray.rs:23` vs `tray-app.js:715`), LIVE state list (`snapshot.rs` vs `core.rs:876` test, `task-controls.js` LABELS).

Not a break: the icon's third counter is ready + blocked by spec (`icon-summary.md:18`).

### Owner decisions

- Freshness: one ttl rule computed in Rust; every 60 s literal goes.
- Counts, states, order, ages, labels: one Rust model; CLI and JS only render it.

### Task 2: one freshness rule

- **Goal:** CLI, tray and menu call the same data live or stale by one ttl rule, and the tray no longer shows false unknowns.
- **Acceptance:**
  1. One Rust predicate decides live/stale for executions, hosts, task observations and `status.json`, from ttl; the page gets the computed state (and ttl if it needs it), never a literal.
  2. Every hardcoded 60 s / 300 s freshness literal in `src/tray-app.js`, `src/tray.rs`, `src/messages.rs`, `src/main.rs` is gone (grep proof in the PR).
  3. Test: an observation 61-299 s old reads live in CLI and tray; one older than ttl reads stale in both.
  4. One live `status.json` on the Mac dev install shows no "unknown" in the tray while `taskq status` reports live.
  5. agentq readback no longer turns stale while the 300 s `runtimes()` probe runs (`src/agentq.rs:143-165`): the probe does not block the readback stamp; first confirm the cause by timing the probe.
- **Scope:** `src/snapshot.rs`, `src/tray.rs`, `src/tray-app.js`, `src/messages.rs`, `src/main.rs`, `src/service.rs` (heartbeat stamp only if needed), `src/agentq.rs`, `tests/tray-running.test.cjs`. Depends on #1578 (`src/tray-app.js`). Separate from #1588.

### Task 3: one display model

- **Goal:** CLI, tray tooltip, menu and icon show the same counts, state names, task order, ages and labels because Rust computes them once.
- **Acceptance:**
  1. One Rust function builds the display model: per-project counts with one dedupe rule, paused/run/watch, state names and order, task order (priority then number), age text, history series names, notification labels, history periods.
  2. `src/tray-app.js`, `src/project-blocks.js`, `src/task-controls.js` render that model and keep no own count, order, label or age logic.
  3. The CLI prints from the same model, including the Paused line `menu.md:249` requires.
  4. Test: one fixture board yields identical counts, order, labels and age text in CLI output and in the model the page gets.
- **Scope:** `src/snapshot.rs`, `src/tray.rs`, `src/main.rs`, `src/notify.rs`, `src/tray-app.js`, `src/project-blocks.js`, `src/task-controls.js`, related tests. Depends on #1578 and on Task 2 (same files).

## 3. Runtime adapter bypasses

The runtime adapter (`src/runtime.rs`, `Adapter` + `trait Proc`) is the one place that knows how Claude and Codex start, run, stop and link. Worker launch, resume, recover, stop, observe, logs and session links already go through it. Bypasses that still branch on the runtime themselves:

1. Owner-session launch copies the adapter: `src/setup.rs:255-342` (`codex_launch`, `codex_thread`, `claude_launch` with its own argv, "backgrounded" parse and refusal checks).
2. Session-link resolution in five copies: `src/service.rs:1365`, `:1550`, `src/tray.rs:485-516`, `src/snapshot.rs:418-440`, `src/setup.rs:345`; two "open session" paths: `src/tray.rs:2010` `open()` and `src/runtime.rs:1786` `app_submit`.
3. Runtime-name branches in the service: `src/main.rs:797-842` `adopt`, `:1883-1888` `lifecycle_waits`, `:2135-2165` `onboard_cmd`.
4. Process lookup and trust edits outside the adapter: `src/setup.rs:464-475` `claude_pids` (pgrep/tasklist), `:399-460` `trust_gap`/`trust` (`~/.claude.json`).
5. The guard in `scripts/check.py` exempts all of `src/main.rs`, and its pattern misses `claude.exe`, `.claude.json`, `CODEX_HOME`, `CODEX_THREAD_ID`.

Why it matters: git history shows about 9 fixes in Claude background-session launch and about 35 in worker liveness; several were a fix in one copy while the other copy stayed broken (#1481/#1496).

Not a bypass: per-runtime sandbox code (`src/policy.rs`, `linux_sandbox.rs`, `windows_sandbox.rs`) is runtime-specific by nature.

### Owner decisions

- Owner sessions (AI request, setup, retro) stay "launch once, the runtime owns it" (taskq.md:173 unchanged); only their duplicated launch code goes. Reason: tracking them would add state in the most fragile area (liveness) for little gain; the session window is already in front of the owner.
- All five bypasses move behind the adapter with no behavior change, and the check closes its gaps so a new bypass fails `check.py`.

### Task 4: runtime knowledge only in the adapter

- **Goal:** only the runtime adapter knows how Claude and Codex launch, link, run and get adopted, and `check.py` enforces it.
- **Acceptance:**
  1. Owner-session launch (`service setup`, tray AI request, `retro`) calls the adapter's launch; `setup.rs` keeps no own argv, output parse or refusal checks. Behavior unchanged: launched once, visible, then the runtime owns it.
  2. One adapter function resolves a session link and one opens it; the five copies and the second open path are gone.
  3. `adopt`, `lifecycle_waits`, `onboard_cmd`, Claude process lookup and `~/.claude.json` trust go through adapter methods; no runtime-name match remains outside `src/runtime*`, the sandbox files and the adapter tests.
  4. `scripts/check.py` scans `src/main.rs` too and its pattern covers `claude.exe`, `.claude.json`, `CODEX_HOME`, `CODEX_THREAD_ID`; a test shows a planted bypass fails the check.
  5. Existing restart/handover and setup tests pass unchanged; one live Mac dev-install restart with a running worker adopts it as before.
- **Scope:** `src/runtime.rs`, `src/runtime/`, `src/setup.rs`, `src/main.rs`, `src/service.rs`, `src/tray.rs`, `src/snapshot.rs`, `scripts/check.py`, related tests. After Tasks 2 and 3 (shared files `src/main.rs`, `src/tray.rs`, `src/snapshot.rs`).

## 4. Churn hotspots

History starts 2026-10-06: 967 commits in 16 days. Most-changed: `taskq.md` (375), `src/main.rs` (134), `src/service.rs` (106), `src/tray.rs` (68), `src/runtime.rs` (68).

| # | Zone | Fixes | Root design reason | Action |
|---|---|---|---|---|
| 1 | Execution liveness ("termination unconfirmed", false gone, stuck doing/starting) | ~35 (#947, #1036, #1386, #1481/#1496, #1551 ...) | No single authoritative execution record; every reader derives liveness from pid, log text, heartbeat age, boot time, board labels. The verdict is still computed in `src/service.rs` `execution_statuses` and `src/snapshot.rs` `execution_status`, plus the tray's own 60 s rule. | Task 5 |
| 2 | Self-update, re-exec, handover | ~25 (#1306, #1357, #1404, #1543 ...) | Handover waits on "are children alive", i.e. zone 1; per-runtime branches in `lifecycle_waits`; tests on fakes. | Task 5 + Task 4 |
| 3 | Tray and workspace UI | ~70 commits, 7 architecture rewrites | UI rules written after the code; template re-render without component state. | None: rules v2, TW1-TW8 and Preact now in place |
| 4 | Icon and history bars | 12 (#798 ... #1587) | Geometry re-chosen per feedback without a fixed constraint; two renderers. | Task 1 |
| 5 | Merge-queue publication | 6 in 3 days (#1352 ... #1537) | GitHub GraphQL fakes written by guess, not recorded; a wrong mutation field name passed tests (#1473). | Task 6 |
| 6 | macOS permission re-prompts (TCC) | 6 (#866 ... #1395) | Unstable binary identity per release path. | None: release-freeze plan (#1395/#1402) |
| 7 | Claude background-session environment | ~9 (#985 ... #1563) | Shared user-level Claude daemon outside taskq's control; launch outcome parsed from CLI text. | None: #1531 open; Task 4 joins the launch code |
| 8 | Several writers of one resource (duplicate log lines, q-label cycle) | ~4 | Each host rewrites the same label; one file is both project log and service stderr. | None for now (small) |

### Owner decisions

- One execution verdict, after Tasks 2 and 4.
- Merge-queue tests on recorded GitHub responses.

### Task 5: one execution verdict

- **Goal:** the service, CLI, tray and handover get the state of an execution (running, waiting, unknown, ended) from one function, so a liveness fix lands once.
- **Acceptance:**
  1. One Rust function takes the execution's signals (child process, native observation, heartbeat under ttl, board labels, boot time) and returns state and reason; `src/service.rs` `execution_statuses`, `src/snapshot.rs` `execution_status` and the handover wait use it; the tray shows its result.
  2. No other code derives running/gone/unknown for an execution (grep proof in the PR).
  3. Table test over the signal combinations behind #947, #1036, #1386, #1481, #1491, #1551 gives the same verdict through every caller.
  4. Spec first if the verdict rules change wording in `taskq.md` § 3.
- **Scope:** `src/service.rs`, `src/snapshot.rs`, `src/main.rs` (handover wait), `src/tray.rs`, tests. After Task 2 and Task 4.

### Task 6: merge-queue tests on recorded GitHub responses

- **Goal:** merge-queue publication tests use real recorded GitHub responses, so a wrong field or state mapping fails before release.
- **Acceptance:**
  1. Recorded GraphQL/REST responses for each merge-queue state taskq handles (enqueued, merged, removed/failed, cancelled, not required) and for the enqueue mutation, from a real repository, secrets stripped.
  2. Publication tests read these recordings; hand-written merge-queue JSON in tests is gone.
  3. A short note says how to re-record when GitHub changes.
- **Scope:** `src/board/github.rs`, `src/board/mod.rs` and their merge-queue tests, new fixture folder. No change to publication behavior unless a recording exposes a bug (then fix it in the same task).

## 5. Overengineering

1. Tray 60 s freshness and the false "Run state unknown": Task 2.
2. agentq readback stale every 5 min (probable, `src/agentq.rs:143-165`): Task 2.
3. Custom icon schema (`taskq tray --icon FILE`, ~200-250 lines, unreachable from the UI): owner removes it; Task 1.
4. `historyRaster` rebin (`src/tray-app.js:8-15`) and Windows seven-segment digits (`src/icon.rs:395-417`): removed by Task 1.
5. Dead weight: ~10.8k lines of generated design previews (`docs/design/agent-settings-preview.html`, `menu-next-preview.html`, `final-menu.html`, `workspace-preview.html`), 13 `docs/design/*.test.cjs` no check runs, `src/enrollment.rs` `coalesce`/`current` behind `#![allow(dead_code)]`, the `scripts/check-web.cjs:12-14` rule that forces ~30 one-lambda wrappers and ~30 noise operations: Task 7.
6. Migration bridges: `src/legacy.rs` (702 lines), legacy paths in `src/host_install.rs`, v4.6.19-22 telemetry budget bridge (`src/telemetry.rs:71-81,134-343`): Task 8.
7. Instrumentation layers (`build.rs` auto-instrument plus `profile::measure`, `tracing::instrument`, `capture::operation`): overlap not confirmed; no task.

### Task 7: remove dead weight

- **Goal:** the repository holds no code, tests or generated pages that nothing runs or reaches.
- **Acceptance:**
  1. Generated design previews nothing runs are deleted (or regenerated on demand by their script); docs links updated.
  2. `docs/design/*.test.cjs` either run in `scripts/check.py` or are deleted; none is left unrun (`custom-icon.test.cjs` goes with Task 1).
  3. `src/enrollment.rs` dead functions deleted and `#![allow(dead_code)]` removed; the build stays warning-free.
  4. The `scripts/check-web.cjs` rule that forces named wrappers is narrowed to module/export level; the one-lambda wrappers in `src/tray-app.js` and `src/resource-controls.js` are inlined; `operations.json` loses the noise entries.
  5. No product behavior change; `python3 scripts/check.py` passes.
- **Scope:** `docs/design/`, `scripts/check.py`, `scripts/check-web.cjs`, `src/enrollment.rs`, `src/tray-app.js`, `src/resource-controls.js`, `operations.json`. `src/tray-app.js` part after #1578 and Tasks 1-3 (same file).

### Task 8: retire migration bridges

- **Goal:** migration code for hosts that no longer exist is gone.
- **Acceptance:**
  1. Evidence from Mac, Windows (native and WSL) and FRA: no pre-host-service units and no 4.6.19-22 telemetry writer remain (read-only checks).
  2. If none remain: `src/legacy.rs`, the legacy paths in `src/host_install.rs` and the telemetry bridge are deleted with their tests; `taskq.md` § 8 legacy steps updated spec first.
  3. If any remain: `ask` with the list of hosts and what still runs there; nothing deleted.
- **Scope:** `src/legacy.rs`, `src/host_install.rs`, `src/telemetry.rs`, `taskq.md` § 8, tests.

## Approved task list

All items owner-approved on 2026-10-22, ready for `taskq add --hold`. Order follows shared files.

| Task | Title | Depends on |
|---|---|---|
| 1 | One icon renderer (SVG, 6 height columns, palette A, no Custom) | #1578 |
| 2 | One freshness rule (ttl, no false unknowns) | #1578 |
| 3 | One display model (counts, states, order, ages, labels) | #1578, Task 2 |
| 4 | Runtime knowledge only in the adapter, enforced by check | Tasks 2, 3 |
| 5 | One execution verdict | Tasks 2, 4 |
| 6 | Merge-queue tests on recorded GitHub responses | none |
| 7 | Remove dead weight | #1578, Tasks 1-3 for `src/tray-app.js` |
| 8 | Retire migration bridges | none |
