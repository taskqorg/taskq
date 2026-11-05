# Conversations refactor acceptance

This checkpoint covers #2366, including the approved numbers-only task timing amendment. The starting source is the merged #2363 result, `b9cbba8f400e42a78bc58e1ef502f46de5b89d7c`. The PR/result identifies the final candidate; this directory travels with its source. No running installation, worker, permission or host policy is changed by these instruments.

R13 was applied before implementation: `taskq.md` Product and the owning reader, layout, persistence and O1 task-row decisions were amended first. R1 keeps the board/native sources authoritative; caches remain disposable read projections. R12 separates browser, local-file, HTTP and native evidence. Section 7.4 keeps the risk protections and records failed reproductions before corrected results. The amended decisions are Product, M3.9, O1, C7 and D4 in their owning contracts; the text of principles R1–R13 and queue/permission rules is unchanged.

## Baseline and costs

Windows, warm build cache. Browser measurements use the bundled page in Playwright Chromium with invented IPC sources; they are single checkpoint observations, not performance distributions or native-renderer measurements. The same instrument was run against an exported immutable baseline. Its initial board request used `start: 0`; the corrected request uses `before: sourceTotal`. Native initial reads were already tail-first.

| History records per source | Baseline first meaningful render | Candidate render | Baseline passage drift | Candidate drift |
|---|---:|---:|---:|---:|
| 12 | 89.7 ms | 95.6 ms | 395.5 CSS px | -0.3125 CSS px |
| 4096 | 110.7 ms | 120.6 ms | 349.0625 CSS px | 0.4375 CSS px |

Both cases made two native IPC reads and one board-history IPC read. The long baseline omitted the latest board page; the candidate retained both recent sources. Selection and focused source control survived in both; only the corrected source passage met the 2 CSS px tolerance. The candidate renders additional correct history, so these measurements do not establish a first-render speed improvement. `node tests/conversations-checkpoint.browser.cjs` also verifies native and board prepend, late dimensions, expanded Details, exact-event priority and explicit Latest.

The local-file instrument uses actual invented files and the pinned AgentQ reader, alternating baseline and checkpoint-reuse branches. Each cold and warm observation makes one native read. Generation sampling reads/hashes 256 bytes per observed read. AgentQ's two full-prefix integrity checks remain; their hash bytes are not directly instrumented.

| Records / file bytes | Warm TaskQ prefix bytes, before → after | Warm TaskQ hash bytes, before → after | Process read bytes, before → after |
|---|---:|---:|---:|
| 12 / 15,240 | 15,240 → 0 | 15,496 → 256 | 45,977 → 30,737 |
| 4096 / 5,201,920 | 5,201,920 → 0 | 5,202,176 → 256 | 15,606,017 → 10,404,097 |

Repeated path resolution changes from scanning 1004 entries in four directories to checking four directory stamps and using the retained exact path. Cache invalidation still scans on tree changes. These are bounded fixture-tree measurements, not constant-cost validation claims. Command: `cargo test --locked --bin taskq conversations_source_cost_checkpoint -- --ignored --nocapture --test-threads=1`.

The read-only live GitHub checkpoint uses closed #2363, one complete trusted projection with 18 history rows and 24,693 serialized bytes. Each observation performs one logical selected-task read. Existing conditional HTTP caching was warm before the probe.

| Read | Baseline HTTP / 304 / billed | Candidate HTTP / 304 / billed | Baseline latency | Candidate latency |
|---|---|---|---:|---:|
| Cold selected projection | 2 / 2 / 0 | 2 / 2 / 0 | 1,416,151 µs | 1,148,846 µs |
| Warm page 1 | 2 / 2 / 0 | 0 / 0 / 0 | 1,136,617 µs | 165 µs |
| Warm page 2 | 2 / 2 / 0 | 0 / 0 / 0 | 1,497,384 µs | 123 µs |

Command: `cargo test --locked --bin taskq conversations_board_cost_checkpoint -- --ignored --nocapture --test-threads=1`, with the explicit project root and closed task supplied privately through its documented environment variables. Reading history makes no LLM-provider call. A 304 still costs an HTTP request; zero billed reads alone would not prove the optimization.

## Failure and retained protection

- Baseline browser checkpoint: long history had no latest board page and passage drift exceeded 349 px. Corrected checkpoint: both tails present, drift below 0.44 px, focus/selection retained.
- Shared source-key regression: emitted `event:7:83` and `owner-request:invented-uuid` now pass the same saved/hydrated grammar; malformed keys remain refused (`reading-anchor.test.cjs`).
- Historical provenance regression failed because Details lacked `Source attempt:71:100:0`. The corrected shared shell includes exact source identity/time and recorded model/version; absent values remain unknown (`conversations-cache.test.cjs`).
- Missing closure-time regression failed before an explicit completed flag: total continued running. It now stays Unknown; closed-state age remains separate (`task-timing.test.cjs`, `compact_timing_uses_first_worker_and_real_changes`). Dependency-derived states without a logged boundary also retain unknown duration/count.
- The extended path regression failed with `unwrap_err()` receiving a replacement path after another session refreshed the tree. Keeping the accepted path separately from its scan proof now refuses the lost source. Duplicate paths, rewrites, replacement and missing generations retain their existing refusal checks.
- The native geometry instrument initially timed out after completing all save/reopen/clamp assertions, during off-main-thread Tao teardown. Its isolated child now exits after assertions; the parent enforces a 15-second deadline and starts a second fresh process. This fixture workaround changes no production shutdown behavior.
- Existing browser instruments needed fixture repairs already exposed on the frozen baseline: unique native record identities replace duplicate undefined keys, whole-row task targets replace retired inline selectors, and the worker refusal label matches the current reader. Caret-only navigation saves retain exact draft identity/text/root and cannot submit, acknowledge or rewrite source/receipt fields.
- The long traversal instrument now measures a visible native paragraph, scopes the prepend announcement to its reader and selects paragraph contents. It observes every mounted page, including intermediate pages evicted before settlement, while retaining ordering, no-omission, residency, stale-reply, selection and keyboard checks. Keyboard setup opens a fresh tail after the selection case, starts near its beginning and focuses a visible Copy control without moving it. This keeps the keyboard check independent of the deliberate ignored-record fill gap and retained-window size. The obsolete hard-cap assertion during selection failed at 5631 retained records; existing D4 explicitly permits disclosed pinned-selection overflow. The corrected check requires that disclosure and unchanged text during selection, and retains the unpinned bound after clearing it.
- The first canonical gate stopped after 253.969 s with 1026 Rust tests passed, four failed and 51 ignored. Two golden fixtures lacked the additive timing projection; regeneration changes only those fields. A cached open fallback hid immediate closure from the exact transcript reader; the owning spec now makes open fallbacks uncached, and the regression requires the next read to see closure while retaining closed revision/TTL checks. Directory metadata lookup had changed the established missing-source error prefix; restoring it retains the refusal rather than returning empty history. All four focused reproductions pass after correction (`cli_and_page_model_agree_on_one_board`, `status_json_matches_the_golden_fixture`, `closed_pin_reads_exact_issue_and_fences_transcript`, `codex_rollout_is_found_and_read`); the expanded selected-cache check also passes. These corrections justify the canonical retry under the documented run policy.
- The second canonical gate passed 1030 Rust unit tests (51 ignored), 21 integration tests (one ignored) and offline selftest, then stopped after 196.438 s on the retired compact single-age markup in `display-model.test.cjs`. The repaired test retains the noncompact CLI-age comparison and requires all three compact fields to remain Unknown when their projection lacks boundaries. The affected dashboard instrument now opens the approved Task info disclosure and checks the new timing suffix without discarding any health, geometry or no-write assertions.
- That dashboard instrument also reproduced a real viewport failure: the wider timing group shifted the health warning left, so its icon-relative tooltip extended outside the window. The tooltip is now positioned against the whole row and bounded by its width. `node tests/task-dashboard.browser.cjs` passes the unchanged viewport-bound, recovery-geometry, source-detail and no-execution-write assertions, plus its normal/narrow, both-theme and large-text cases. `node tests/display-model.test.cjs` also passes after the markup repair.
- Independent review then identified a hover-path regression from that row anchor: moving from the warning to a wrapped title hid the tooltip. The isolated browser reproduction failed before the row-hover selector and passed after it; the dashboard now retains warning-to-title-to-tooltip pointer assertions alongside keyboard focus. The preceding canonical candidate passed all layers in 817.641 s; the final gate is repeated because this last shared CSS change must also be verified.

## Acceptance coverage

| Item | Evidence and scope |
|---|---|
| A1 | Immutable baseline and short/long render, drift, bytes, native reads and HTTP/304/billed checkpoints above. Native first-render latency is not measured. |
| A2 | Shared grammar/mark/hold; checkpoint and existing selection/stability instruments cover append/prepend, streaming, delayed sizes, Details, exact targets, switching, focus/selection, drafts and explicit Latest. Chromium/WebKit proof is separate from native input smoothness. |
| A3 | Initial board tail accompanies native tail; bounded 64-record board pages, previous pages and accepted receipt source fences. `conversations-history.browser.cjs` retains its 100k-history and current-question-singleton assertions. |
| A4 | Shared selected-task projection tests cover concurrent reads, revision/TTL expiry, negative reads, eviction and exact worker identity. Live warm reads make zero HTTP requests. Complete trusted fold remains the authority. |
| A5 | Local-file costs above; deep rewrite/replacement and cross-session lost-path regressions retain integrity protection. No AgentQ API/parser change. |
| A6 | `conversations-layout.browser.cjs`: pointer/keyboard divider, ordinary resize, wide hide/show, narrow restoration, saved preferences/reload and long draft at 14/16/18/20 px. |
| A7 | Shared native window builder and additive settings tests cover logical geometry, current work area, scale and unavailable-display fallback. Isolated Windows native process/reopen results are recorded with final verification below. No production service restart is performed. |
| A8 | One history shell retains roles, time, Copy and source-backed provenance/unknowns; sticky header has no persistent inner scroller. Native screenshots and final verification are listed below. |
| Timing amendment | Rust trusted-projection tests cover first confirmed worker ownership, waits/rework, repeated same-state entries and unknown/unstarted/completed history. `task-timing.browser.cjs` checks actual Home/Conversations rows at normal/narrow widths and 14/16/18/20 px, all three values, icons, full-width title, accessible description and no per-number tab stops. |

The #2390 note is not treated as proof of a reload cause: its pre-reload semantic anchor was absent and an attachment picker was also open. This task's controlled passage tests do not establish that picker's causality.

## Final verification

The one pre-push rebase used `6c37fbfc4ff14af652760b2219df6bd04c24c0bc`; no subsequent rebase is used. The final canonical gate is recorded below when it completes.

`node tests/conversations-render.browser.cjs` passed both 10,000-record sources. Unpinned maxima were 600 records; selected-range maxima were 5631 Claude items / 3756 Codex items, with visible overflow notices and retained text. This existing selection exception can increase DOM work; it is not a hard memory-bound claim while text remains pinned. In this run, receive/render p95 was 607 / 424 ms and wheel frame p95 was 17 / 17 ms; maxima were 200 / 150 ms. These are browser fixture observations, not native smoothness acceptance.

`cargo test --locked --bin taskq conversations_geometry_native_reopen -- --ignored --nocapture --test-threads=1` passed in 2.04 s. Real Windows windows retained logical position `[100, 80]` and client size `[760, 500]` at scale 1 through same-process close/reopen and a second fresh process loading the saved settings. An invented missing-display identity restored inside the actual current work area. This verifies restart-persistent settings, not a restart of the production service.

`cargo run --locked -- tray --snapshot .taskq/2366/native-checkpoint-receipt --route conversations-checkpoint` passed on Windows WebView2 with an isolated install and invented IPC sources. The fixture asserts paragraph drift within 2 CSS px, retained selected text and focused Copy after a 120 px late size above the passage and expanded Details. Both themes returned an explicit completed assertion receipt: drift `0`, selection `true`, focus `true`. Missing completion fails the snapshot; asynchronous callback errors cannot silently qualify it. It makes no board, session or provider call. Captured binary SHA-256: `d3d749efad0e0e9bd25392567b62054ecf9d03640728e1502d469f17ac41e25b`; its source is the candidate carrying this report. Both natural-size captures were inspected:

- [Dark native shell](native-checkpoint-dark-16.png) and [light native shell](native-checkpoint-light-16.png): role, time, model, Copy, selected passage, expanded provenance with recorded version/exact source and explicit unknown effort, divider and panel-edge scrollbar.
- Browser-only [Home 350](tray-timing-350.png), [Home 620](tray-timing-620.png), [Conversations 350](workspace-timing-350.png) and [Conversations 1100](workspace-timing-1100.png): the same three timing values, icons and full-width title. The narrow sidebar wraps metadata; the normal-width Home row retains one metadata line. Unknown source/health notices are intentional fixture data.

Source/build verification does not qualify macOS, physical monitor removal, mixed-DPI hardware or native wheel/trackpad momentum. Those platform cells remain explicitly untested. No live deployment is claimed.
