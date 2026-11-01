# Clear agent totals and joined Held admission

Task [#2099](https://github.com/taskqorg/taskq/issues/2099) authorizes Product, M3 Running agents, K-A3/K-A6 and D-A11/D-A13. The [owner screenshot](https://github.com/user-attachments/assets/be5bbcb6-30ca-40d2-a4b0-94a96113a214) and [two-zone instruction](https://github.com/taskqorg/taskq/issues/2099#issuecomment-6553907123) were already attached before the recorded Start. The implementation preserves the pinned visual reference outside these corrections (R13).

## Failure and source trace

Baseline: `origin/main` at `feb1b74a6f36a9f8b5107a9909de942ed634f7cd`.

The pictured `Ali ? 47m`, `mac 2`, `pds 0`, `win 0`, Codex `1+?` and Claude `1+?` follow `tray.rs` freshness projection → project `complete/tasks_live/observed`, heartbeat `live/age/age_text` and fenced task `executions` → `tray-app.js::runningAgents`. The collector's freshness flags remain authoritative. The renderer deduplicates executions by board/task/execution, groups Codex-app into Codex and excludes conflicting hosts from confirmed totals. Any stale/unread source makes the overall family totals incomplete; the old formatter appended `+?` to each runtime, including a zero subtotal. This is display uncertainty, not extra known agents, capacity or sign-in state. The historical screenshot's raw snapshot is unavailable; its exact execution identities remain unknown (R12).

Before implementation:

- `node tests/tray-running.test.cjs` failed: actual `2+?`, expected `≥2` for a confirmed lower bound with an explained group state.
- `MENU_2099_BEFORE=1 node tests/menu-held-control.browser.cjs` against the baseline bundle failed: Doing-only had 0 joined controls, expected 1. Start was derived from state-filtered rows and omitted when drafts were hidden.
- The host-strip geometry check exposed Attention at width 0. A later general summary selector overrode its three-column grid. The specific host-strip selector now preserves Attention.

The synthetic browser fixture mirrors two confirmed agents on mac and a 47m stale host. It uses the unchanged `icon::css` dark/light palette and bundled icons. All bridge messages are captured locally; no live Held task is started.

## Implemented and checked

| Acceptance | Evidence and result |
|---|---|
| A1 | Product, M3.1/M3.24, K-A3/K-A6 and D-A11/D-A13 amended before code. State meanings, source IDs, authority and revision fencing unchanged. |
| A2–A3 | `tray-running.test.cjs`: fresh/idle, partial, stale/missing/invalid data, conflicting observations in both orders, deduplication, ended/waiting executions, Codex-app, other runtimes, resources and separate sign-in warnings. `≥N` is a nonzero confirmed minimum; incomplete zero is `?`. One Partial/Unknown label explains the group. Host summaries expose age and cause with host/runtime totals. |
| A4–A5 | `menu-held-control.browser.cjs`: two sibling buttons, divider, Doing-only and OR filters, 0/1/5/10 targets, pinned drafts, search and canonical mixed-project identities. Start names count, projects, query and view. All tasks and per-task Start remain independent. |
| A6 | Same browser instrument: frozen revisions, changed-revision refusal before IPC, double confirmation, one pending send, refresh, stale/failed/unknown/accepted receipts, ready/waiting readback and same-operation retry. Unknown/zero scopes are disabled. No focus, capacity or PM operation is sent. Existing Rust/CLI receipt enforcement remains unchanged. |
| A7–A8 | 24 width/font/theme cells (320/360/470px × 14/16/18/20px × dark/light), 44px touch targets, keyboard order and clicks immediately on both sides of the divider. No horizontal overflow; joined segments share a wrapping line. Existing state-filter, host-strip, dashboard and panel instruments protect adjacent paths. |

`tests/tray-state-filter.browser.cjs` now supplies the required display fixture and selects the project through the current Home view model; it retains OR, keyboard, refresh, scoped All and same-ID retry checks. Old host/panel assertions now follow the already shipped Completed → Conversations and runtime → task-agent view, with the exact native URL tested separately. No product behavior was changed to restore obsolete test expectations.

The old wheel instrument also failed on the baseline at its 1500ms gesture wait. Diagnostics identified a 320px/20px coarse-pointer row whose bottom edge could lie below the viewport. The fixture brings that edge above the footer before sending the real wheel event; both directions, 90/12 deltas, all boundary targets and the original deadline remain protected.

## Visual evidence

Browser-only, macOS host, Node 22.18.0, Playwright 1.64.1 Chromium headless shell, device scale 1. The client platform field is illustrative, not installed native qualification. Source/build and canonical-gate receipts are recorded in the PR and task result (§7).

| Scenario | Before | After |
|---|---|---|
| 320px, 16px text | [Narrow](before-narrow.png) | [Narrow](after-narrow.png) |
| 360px, 20px text | [Large text](before-large-text.png) | [Large text](after-large-text.png) |

Additional distinct states: [named stale-host details](host-details.png), [frozen admission review](admission-review.png). The images were opened and inspected at natural size: host identities and total meanings remain readable; Held and green Start remain adjacent with separate targets; other counters, All tasks, view picks and task navigation remain present. The intentional changes are the explained minima, joined Held/Start and Attention grid correction. There is no new permanent row, theme, icon family or telemetry/admission policy.

Native macOS/Windows acceptance remains **Not tested**, independently in the owner chat. Merge-result `merge_group` CI remains the publication gate (R12, §7); browser and source checks do not replace it. No installation, service restart, grants, capacity changes, owner answers or live batch admission were performed.

## Rule-gate authorization

The first canonical run stopped after 4.026s: A1 named M3 Running agents, but the literal rule-ID gate required `M3.1` and `M3.24`. The [owner answer](https://github.com/taskqorg/taskq/issues/2099#issuecomment-6554541300) explicitly authorizes both IDs and requests resumed checks and normal review/publication. No guard or task body was changed. The repeated canonical run includes the full implementation diff against `dd695b8d1ad2fec03853ea4c6edff3d66faf7acb`; its result is recorded in the PR and task result (R12/R13, §7).
