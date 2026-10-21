# App foundation · #1440

Owner-selected target: Preact + Signals + Wouter for the TaskQ app shells, with native snapshot delivery and native persistence retained. [Owner answer](https://github.com/taskqorg/taskq/issues/1440#issuecomment-6309805829) selects option 1 of the ask (option 2 in the comparison below). The owner's #1463 scope folds M3/M4 into #1463–#1467, with M2 running in parallel and M5 after those tasks (R13, § 7.5). This specification does not establish runtime qualification.

Inspected candidate: `377a424d4907120e7c5ddb0b1032c68ddaaf7c76`. The primary checkout has separate edits; this inventory describes the task worktree. Inventory and package-size evidence were retained from the earlier attempt of #1440 and checked against the continued worktree. They are research observations, not measurements of the selected implementation. Native latency, memory and CPU were not measured (R12).

## Current mechanisms (A1)

| Source | Routing and state | Snapshot/cache and rendering | Persistence |
|---|---|---|---|
| `src/tray-app.js` | `createState` mutable view object; `navigate`, `back`, `remember` and private `trail` stack; page, filter, work card, drafts and operation maps are separate fields | `receive` distinguishes snapshots and receipts, replaces `view.S`, reconciles operation readbacks. `draw` builds HTML strings, skips identical HTML, otherwise replaces `menu.innerHTML`, remounts cards and restores focus, caret, disclosure and scroll state. Refresh waits during presses (up to 5 s), resize and selected editing states | Navigation and temporary controls are in memory. Settings use `set` IPC; AI text uses `ai-draft` IPC. No localStorage or browser task database in this file |
| `src/ui-runtime.js` | CoreQ execution context, instrumented listener wrappers and event context inheritance; not a view store or router | `send`, `connect`, `listen`, `defer`, `cancel`, `effect`; native observations and effect completion including unknown outcomes. No query cache or renderer | No collector/storage here; native capture writer owns observations |
| `src/workspace.js` | Closure variables for snapshot, selected conversation, draft, pending, error, search and list visibility. Click-driven selection, no URL router | `taskq.receive` replaces snapshot, processes receipts and selection; `render` replaces the workspace HTML and restores transcript scroll, focus and textarea selection. Search also renders the whole workspace | Draft/select/send/confirm use IPC; `src/conversations.rs` atomically saves preferences/drafts/selection and reads request history. Pending is separate from authoritative outcome |
| `src/project-blocks.js` | Stateless exported rendering functions and adapter-driven `handle` navigation; no independent app state or router | Canonical rows in, escaped HTML out; identity deduplication, partial/conflicting data, separate Doing/Review, unknown Closed. Consumer owns insertion and refresh | None; versioned shared assets are released for browser consumers |

The four files total 109,988 source bytes, not a production bundle size. `src/tray.rs` already compares snapshot signatures, excludes tray `now` from the comparison and avoids another snapshot call while one is pending. A client query library would not remove this producer work. `build.rs` already invokes `scripts/build-web.cjs`; esbuild emits ES2022 IIFEs embedded in HTML. Adding Preact does not require replacing wry, the native shell or the build system.

`scripts/check-web.cjs` requires named app functions, methods, events and async work to use CoreQ execution boundaries. Standard component syntax is not automatically compatible. Integration must retain operation inventory and trace propagation rather than exempting all new UI code. Library internals are not product operations.

## Options (A2)

Sizes below are a reproducible exported-API probe, not complete apps, runtime benchmarks or promised savings. Raw minified bytes matter for embedded HTML; gzip is included only for comparison. The vanilla option retains existing renderer/router code, so its small dependency does not imply the smallest final application.

| Option | Added library probe: minified / gzip bytes | Update behavior; actual wry speed unknown | Persistence and offline | Test cost / migration scope |
|---|---|---|---|---|
| 1. Vanilla + Nano Stores | 1,902 / 980 | Atomic subscriptions centralize state; full HTML rebuild remains unless separately rewritten | Keep native draft/settings writes and snapshots; no automatic durable store. Offline shell works; stale board rows remain partial/unknown | Lowest initial migration: state subscriptions in both shells. Existing browser tests remain; manual focus, reconciliation and routing mechanisms remain maintenance obligations |
| 2. Preact + Signals + Wouter | 27,092 / 10,822 | Keyed components retain DOM identity; signal subscriptions limit affected updates. Gains depend on component boundaries and changed snapshots | Native persistence retained; signal snapshot is a disposable in-memory cache. No offline mutation replay or browser task store | Medium: convert both shells by route, retain shared asset islands and IPC tests. Add component behavior checks where existing browser checks cannot detect failure |
| 3. Solid + Solid Router | 33,006 / 12,884 | Fine-grained dependency updates with compiled views; no application-specific latency proof | Same native persistence and offline policy as option 2; signals/stores do not supply durability | Medium-to-large: both shells rewritten, JSX compiler integration and instrumentation compatibility work. Preserve existing browser and native acceptance |
| 4. Solid Router + TanStack Solid Query + Solid | 75,183 / 25,474 | Adds keyed query cache, invalidation and observers; does not make pushed full snapshots smaller. Useful if independently fetched resources emerge | Query persistence requires a separate persister; not included in probe. Configure local IPC independently of network online state; forbid persisted/replayed task mutations | Largest: option 3 plus query keys, invalidation, staleness, retry, reconnect and hydration policy tests; overlaps current native snapshot owner |

Probe uses esbuild 0.28.2, browser conditions, ES2022, ESM, minification, retained exports and gzip level 9. Versions: Nano Stores 1.5.5; Preact 11.0.1; Signals 2.11.3; Wouter Preact 3.13.0; Solid 1.9.17; Solid Router 1.0.0; TanStack Solid Query 5.104.5. These are research inputs, not dependency selections or compatibility qualification. The existing release build is not minified; its eventual raw artifact must be measured separately.

Exact probe imports and output are in [app-foundation-size-evidence.json](app-foundation-size-evidence.json). All imports remain exported to avoid an empty tree-shaken bundle. There is no UI fixture or emitted JSX in the probe; application code, compiler output, existing CoreQ, shared assets and persistence plugins are excluded. Installation used `--ignore-scripts`; no project package or lock file changed.

## Selected architecture (A3)

Option 2 is selected for standard rendering and state ownership; native speed relative to Solid is unknown. A store alone leaves the observed manual DOM preservation code in place. A query stack adds a second freshness and retry policy to a push-fed client. Preact fits the existing esbuild pipeline and supports gradual component conversion without a JSX compiler requirement (`h` is sufficient initially).

- View state: Signals for drafts, selection, pending operation identity and per-route controls. Keep receipt/revision reconciliation explicit; framework state never substitutes for native readback (R1, R12).
- Routing: Wouter memory location for the wry shell. Back navigation retains scroll/disclosure state per history entry. Project identifiers remain data, never URLs opening arbitrary native navigation. Browser consumers may supply a browser/hash location adapter; this task does not change DashQ routes.
- Data: one latest accepted native snapshot per window in memory, derived selectors for components, batched snapshot application. Preserve snapshot completeness, observation age, unknown values and existing receipt fences. Do not manufacture fresh board timestamps on render. No second board client, query poller or durable task cache.
- Rendering: stable keys for project, board/task and conversation identity. Keep inputs mounted across snapshot updates. Shared answer/resource/project blocks initially remain isolated DOM islands with their existing APIs; only the island owns its descendants. This boundary still needs its existing focus/click tests.
- Persistence: retain Rust ownership of drafts and preferences, including view-only route/filter/scroll/disclosure restoration across reload (V05/V06). The #1427/#1462 decision supersedes the historical conversation inventory: owner sessions live in their runtimes, with no TaskQ conversation store. UI writes use existing IPC protocols; failure stays visible and unknown remains unknown. No localStorage copy of task truth and no browser outbox.
- Offline: local assets and retained native data can render; disconnected/stale board observations remain partial/unknown. Preserve existing native operation capabilities and refusals. Never add generic automatic retries, optimistic task completion or mutation replay after reconnect.
- Telemetry: keep `ui-runtime.js` and CoreQ as the execution boundary. Route component handlers, effects and timers through the current contract; qualify parent/child context through scheduled framework work. Do not add a second collector.

### Contract compatibility and CoreQ ownership

The [project-blocks contract](project-blocks.md) explicitly adds no framework, network client, credentials, parser or task operation. Retain that rule for the exported assets: a Preact app shell may consume their existing functions through an island without bundling Preact into their public artifact. Do not reinterpret the rule as permission to rewrite project-blocks in Preact. If a future component rewrite changes that boundary, it needs its own owner decision and contract edit (R6, R8, R13).

Keep the app foundation in TaskQ initially. CoreQ already supplies execution/telemetry and native release infrastructure; it should not acquire TaskQ routes, board state or a mandatory UI framework. Reuse by DashQ is currently demonstrated through the project-blocks asset contract, not through a verified common app shell. DashQ implementation was not inspected because this worker is scoped to TaskQ. Propose a separate optional CoreQ UI package only after both consumers demonstrate the same independent API; shared policy belongs in specq, project behavior remains here. No cross-repository changes are authorized by this decision.

### Performance observation

Performance is watched through existing CoreQ telemetry. The [owner's #1463 steer](https://github.com/taskqorg/taskq/issues/1463#issuecomment-6314418072) excludes benchmark scripts, performance tests and new latency/memory ceilings from this and later UI PRs (R13). Keep current execution context and collection boundaries; add no second collector or performance harness. No CPU, memory or latency improvement is claimed by the architecture decision.

M2 observation runs in parallel and is not an implementation gate. UI geometry, interaction, functional checks and matching native-renderer qualification remain separate under R12; they do not become performance benchmarks or latency/memory budget approval. Per [specq observe, do not gate](https://github.com/taskqorg/specq/blob/main/engineering/performance.md#observe-do-not-gate), any budget or ceiling, including 70% sustained CPU, is a telemetry reference value: never task acceptance, a merge gate or an owner ask. A degradation the report flags becomes its own task. Existing host policy and runtime authority remain unchanged.

### M2 baseline (#1469, one-time note)

Measured once on main with a throwaway tray route and script that were not merged. Each fixture ran twice back to back with one binary, so the pair shows run-to-run noise. Hold 60 s, one step every 250 ms (refresh, typed character, click to Settings, click to Home), about 56 samples per step kind and run. Values are the worse of the two runs, in ms, from cause to the second animation frame; CPU is percent of one core over the hold after the first 5 s; memory is taskq plus its webview processes.

| Host | Fixture | Snapshot p95/p99 | Typing p95/p99 | Click p95/p99 | CPU mean/p95 | Memory peak | Errors |
|---|---|---|---|---|---|---|---|
| macOS 27.0.1, Apple M2, 8 cores, WKWebView | small | 39 / 49 | 32 / 58 | 34 / 65 | 16.4 / 19.3 | 123 MB | 0 |
| | large | 50 / 66 | 48 / 78 | 65 / 67 | 23.4 / 27.3 | 200 MB | 0 |
| Windows 11 26300, i5-12600KF, 16 threads, WebView2 155.0.4283.62 | small | 31 / 39 | 8 / 9 | 10 / 14 | 11.7 / 17.0 | 454 MB | 0 |
| | large | 51 / 56 | 6 / 8 | 36 / 41 | 21.7 / 30.2 | 576 MB | 0 |

The large fixture is 12 projects of 25 tasks and 20 AI requests; small is 2 of 4 and 2. JS heap is 9.5 MB on WebView2 and unknown on WKWebView. CPU stays far below the 70% reference value on both. An earlier Windows run had one large-fixture snapshot p99 of 306 ms against 55 ms in its pair: single p99 values are noisy. Unknown: the workspace (not on main), OS-level input dispatch and IME (events were synthetic), physical press/release, a locked or occluded window, Linux.

Telemetry gaps: CoreQ telemetry records `tray.refresh`, `tray.web.delivery`, `tray.web.error` and the tray operation spans, but no span covers snapshot-to-paint or input-to-paint, and no record samples the CPU or memory of taskq and its webview processes. Until such spans exist, these values read unknown in telemetry.

## Migration task list (A4)

The architecture decision is accepted. The owner folded M3/M4 into the five approved UI tasks #1463–#1467: each task builds its own supported screens directly on the selected foundation. #1463 establishes the shared foundation once, with pinned dependencies and the existing esbuild packaging. There is no separate conversion prerequisite. M2 runs in parallel and does not gate implementation; native rollout remains gated on qualification evidence (R8, R12). Keep existing durable formats so each slice can roll back independently; rollback cannot undo operations already sent to the service.

| Order / dependency | Outcome and scope | Acceptance and retained protection |
|---|---|---|
| M1 · this specification | Record the owner-selected foundation, inventory, options and shared-asset/CoreQ boundaries | A1–A4 of #1440; root contract and design index link here; no runtime or dependency changes |
| M2 · parallel after M1 | Observe the workspace and tray through existing CoreQ telemetry | No new benchmark scripts, performance tests or latency/memory ceilings; observations stay platform-labelled and unknown remains unknown. This is not an implementation gate |
| M3/M4 · folded into #1463–#1467 | Build navigation/controls (#1463), answers/requests (#1464), operation reviews (#1465), menu panels (#1466) and messages/archive (#1467) directly on Preact + Signals + Wouter | #1463 supplies the shared shell and routing handoff; later tasks implement their own screens. Preserve native snapshot/receipt ownership, drafts/IME/caret, Back/scroll/disclosures/focus, press-refresh, duplicate refusal, exact revisions and unknown/late receipts. Public project/answer/resource islands remain framework-free. Performance stays in existing CoreQ telemetry; browser interaction evidence and each native renderer stay separate. M2 runs in parallel, not as an implementation gate |
| M5 · after #1463–#1467 | Remove obsolete shell mechanisms and qualify the resulting artifact for release | Run selected canonical gate and ordinary publication CI; keep screenshot manifest/provenance, public asset hashes, CoreQ tracing and both native platform checks. No unused second store/router/cache, browser outbox or automatic mutation replay remains |

No test or assertion is changed or weakened in #1440. This is a specification delivery, not a runtime fix: there is no failure-before/pass-after speed claim. Implementation tasks must record before/after evidence at their affected boundary and follow § 7 publication gates. CoreQ extraction is deferred until TaskQ and DashQ demonstrate the same independent API; it is not part of this migration list.

## Primary sources

- [Preact](https://preactjs.com/) and [Signals](https://preactjs.com/guide/v11/signals/): component and reactive update model.
- [Wouter](https://github.com/molefrog/wouter): Preact support and memory location.
- [Nano Stores](https://github.com/nanostores/nanostores): atomic stores and subscriptions.
- [Solid fine-grained reactivity](https://docs.solidjs.com/advanced-concepts/fine-grained-reactivity): dependency-driven updates.
- [TanStack Query network modes](https://tanstack.com/query/latest/docs/framework/react/guides/network-mode): offline/online behavior; framework-specific integration still needs qualification.
- [TanStack Router history types](https://tanstack.com/router/latest/docs/guide/history-types): memory/browser/hash alternatives, considered as a heavier routing alternative.

Upstream descriptions establish available mechanisms, not TaskQ performance, compatibility or adoption.
