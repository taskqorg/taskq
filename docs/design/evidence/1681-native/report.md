# Menu and Settings finish: #1681

The candidate implements the accepted #1657 corrections without changing product decisions, queue policy, grants or runtime settings. Applied contract: R6, R8, R12, R13 and § 7; design rows R05, R07, R16–R18, R28, R32, R34 and R43. No R-number is amended.

## Candidate and proof boundaries

Rebased UI source: `a87509c15486135cd55f8c2c2060cf907c5d93f2`; immutable asset hashes and the native theme matrix are in [matrix.json](matrix.json). README images and the signed TaskQ binary are pinned separately in [the screenshot manifest](../../../shots/manifest.json).

- Browser: the bundled product shells in Playwright Chromium headless shell, macOS arm64, Node 22.18.0. Component checks retain unknown/stale handling, safe session targets, account/method separation, current-versus-captured sign-in, exact runtime identity, and recovery/lifecycle receipt fences.
- Native: macOS 27.0.1 arm64, Wry/WKWebView, 18 px, light and dark. A temporary signed Cargo example loaded the exact `build/web/tray.html` and `workspace.html` bundles and discarded effect requests. Its assertions required no JS errors, no horizontal overflow and no clipped route title in all 20 route/theme cells. It was removed after capture; no new application, dependency or test infrastructure ships.
- The native fixtures reuse the Settings and host-resource browser fixtures. All hosts, accounts, tasks, versions and resource readings in the committed images are invented. The displayed `4.1.0` is a fixture value, not the candidate version. Screens were visually inspected; matrix assertions alone are not visual acceptance.
- The signed product also rendered the actual project's read-only status through `tray --snapshot --route home,hosts,agents,services,cadence`, both themes at 16/20 px. Those local observations and account/session data are not committed. Snapshots use an isolated installation and do not apply controls or contact the board.

This is rendered-source/native snapshot evidence, not deployment, new sign-in probes, or native command-effect qualification. Windows/DPI, native drag/z-order, actual session raising, and live Stop/Recovery effects are **Not tested**. R12 keeps those claims unknown.

## Failure before and pass after

| Defect | Before | After / retained protection |
|---|---|---|
| R05 compact runtime action | `node tests/project-blocks.test.cjs`: `verified local mark opens its session` failed against the original implementation. | Same check passes. Unsafe URLs and remote-local Codex targets remain refused; unavailable marks retain a disabled reason. `tray-panels.browser.cjs` activates the mark and checks the exact dispatched URL. |
| R07 title measurement | `window-chrome.browser.cjs` with the original shell timed out waiting for a non-fitting title to move below after the menu narrowed from 800 to 280 px at 18 px. | Same scenario passes after layout/width/font measurement. Widening restores the inline title; shrinking again preserves action coordinates. Ordinary 320/360/800 px navigation and Back/Close checks remain. |
| R16 collapsed hosts | `tray-running.test.cjs` expected named hosts and both counters; original output was `4 hosts` / `Runs unknown`. | Same check passes. Worker/reviewer identity, duplicate boards, Codex-family totals, stale/partial observations and reservations remain covered. Attention coexistence is checked in `tray-panels.browser.cjs`. |
| R18 exact clients | Original compact rows called both clients Codex and hid versions. | `tray-settings.test.cjs` and `settings-content.browser.cjs` distinguish Codex and Codex app in Agents/Access, expose versions and preserve remote Unknown and AS5 states. |
| R28 narrow summaries | First native candidate squeezed host/admission summaries into a trailing column. | Final Settings screen gives these summaries the full text row; no type or control size is reduced. |
| R32 effective admission | Original Settings counted focus/run projects as `admitting`, even with access refusal. | The actual workspace resource route leads with scoped Unknown/Mixed/Blocked work; configured intake is separate. `tray-resources.browser.cjs` checks an open load gate plus access refusal before the capacity widget. |
| R34 Activity disclosure | First candidate opened nested exact timestamps/source diagnostics automatically and squeezed the new icon/period into the wrong grid cells. | `tray-history.browser.cjs` checks initially closed diagnostics, an in-bounds period, six selectable canonical bins and preserved incomplete/lower-bound quantities. |
| R43 workspace overflow | Native [before](workspace-before-dark.png) clipped navigation beyond the left edge. | [After](workspace-dark.png) anchors the same menu to its header. `window-chrome.browser.cjs` checks its bounds and bundled vector. |

Changed tests follow SpecQ Testing: expectations changed only for the accepted presentation correction. No queue, ownership, recovery, URL-safety, incomplete-data, temporal-bin or lifecycle protection was removed.

## Per-requirement disposition

Pass is limited to the source, browser and native rendering boundaries above.

| Requirement | Status | Evidence |
|---|---|---|
| R05 / A3 | Pass | Labelled enabled/disabled runtime marks; exact dispatch browser check; [menu](home-dark.png). |
| R07 / A1 | Pass | Post-layout/resize browser reproduction; all native title-cut cells false; [menu](home-dark.png). |
| R16 / A2 | Pass | Named host/family counters, Attention coexistence and unknown/dedup checks; [hosts](hosts-light.png). |
| R17 / A5 | Pass | CPU/memory meters retain percent units/sample age; count definitions and resource diagnostics disclose; [hosts](hosts-light.png). |
| R18 / A4 | Pass | Exact clients and versions, account/login freshness, remote Unknown; [Agents and Access](agents-dark.png). No new remote probe is claimed. |
| R28 / A5 | Pass | Full-width long summaries and disclosed exact targets; [Settings](settings-light.png), [Services](services-dark.png), [Work mode](workmode-light.png), [Recovery](recovery-dark.png). Browser checks retain fresh recovery review, selected tasks/evidence and same-payload reconciliation. |
| R32 / A6 | Pass | Scoped effective facts precede separate configured intake; access is not host-wide refusal; [Work mode](workmode-light.png). |
| R34 / A7 | Pass | Concise event quantities, truthful lower bounds, one six-bin interval selector and disclosed exact source; [Activity](cadence-light.png). |
| R43 / A7 | Pass | Bundled History/overflow icons, existing icon inventory validation and [workspace](workspace-dark.png). |
| A8 | Pass | Browser checks plus 20 native theme cells. This review set has exactly 10 screens: nine distinct surfaces and one before-failure screen; theme variants remain matrix coverage. |

## Verification

Development checks: `project-blocks.test.cjs`, `tray-running.test.cjs`, `tray-settings.test.cjs`, `window-chrome.browser.cjs`, `settings-content.browser.cjs`, `tray-panels.browser.cjs`, `tray-history.browser.cjs`, `tray-resources.browser.cjs` and `tray-recovery.browser.cjs` passed after the applicable fixes. `docs/shots/update.sh` refreshed the native README images; source blob and signed binary hashes were read back against its manifest.

The final canonical gate ran after rebase on `a87509c15486135cd55f8c2c2060cf907c5d93f2`: `python3 scripts/check.py --base 58c86663896048666a836c84c269873ce931ae1f`. It selected full checks and stopped in Rust unit tests after 43.319 s: **532 passed, 17 failed, 32 ignored**. Record **sandbox-blocked**, not PASS: host-operation tests encounter the worker guards `a worker cannot change host policy`, `a worker cannot install the host service`, and the corresponding Stop/restart refusals. Related expected-success assertions also fail in that worker environment. No worker guard or host test was weakened. Rust integration stages did not run; their outcome remains unknown pending CI.

The remaining cheap checks ran separately: 24 Node component scripts, three artifact/release-script checks and all six changed browser instruments plus the upstream task-dashboard browser instrument. Two consumer provenance checks initially found stale hashes in `finding-composer.sources.json` and `task-workspace.sources.json`; refreshing their transitive component hashes made the same checks pass, preserving every integrity assertion. All 24 Node scripts and all seven browser instruments passed on the rebased candidate. An earlier pre-rebase gate had 529 passed, the same 17 failures and 32 ignored. It was repeated because upstream #1683 changed Rust and the shared UI bundle. Native capture and browser checks were also refreshed after rebase; no new Rust failures appeared.

Offline `cargo run --locked -- selftest` passed 97 golden traces and format checks, and committed nine telemetry observations in an isolated temporary store. Board status is explicitly unknown/offline; no board-read success is claimed.

Publication/merge-result CI is separate under R12 and § 7; this worker does not claim merge or deployment.
