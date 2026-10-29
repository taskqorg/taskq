# Whole-window opacity verification · #1812

The owner approved stronger translucent text in [answer 2](https://github.com/taskqorg/taskq/issues/1812#issuecomment-6490519452). The [design note](design.md) compares the native routes. R13 is applied through the new Product Menu opacity decision, M1.1 and the translucent text exception; R1–R13 and existing guarding assertions are unchanged.

## Before and after

- Before implementation, `node tests/tray-settings.test.cjs` failed with `Appearance must expose menu opacity`. The same check now passes, retaining its existing sign-in, access, route and recovery protection.
- The prior light Work color gives a calculated 3.96:1 at 95% opacity on independently adverse backdrop pixels. The retained contrast check verifies strengthened primary, secondary and semantic colors at every integer opacity from 90% through 99%, in both themes, on body/header/selected/popover surfaces, using both floating-point and Windows byte alpha. Each pair must be at least 4.5:1. Off retains the original palette.

The first canonical gate also caught the root contract exceeding 60 KiB and the new opt-in snapshot route entering the default-catalog count. The root now links M1.1 within its existing byte/line limits. The snapshot check retains the prior default matrix and adds the exact six opacity cases. Neither size guard nor prior snapshot coverage is weakened.

## Checks and limits

| Check | Observed result |
|---|---|
| `cargo test --locked --bin taskq tray::tests::opacity` | Pass: 2 tests; value validation, file reload, unknown-key preservation, state readback and composed color-pair contrast. |
| `node tests/tray-settings.test.cjs` | Fail before; pass after. |
| `tests/*.test.cjs` (all 25 component files) | Pass after refreshing current source-provenance manifests, including their dependency on the changed visual-style specification; assertions remain intact. |
| `node tests/tray-opacity.browser.cjs` | Pass: keyboard range/Off IPC, route adoption, shared body tokens, visible refusal, opaque Settings and narrow/large-text layout. |
| `cargo run --locked -- tray --snapshot DIR --route opacity` with isolated fixture | Native macOS alpha readback matched all six Off/95%/90% cases. Composition capture is sandbox-blocked: `Screen Recording not allowed (TCC); no prompt requested`. Exit 1 is not a pass. |
| `cargo run --locked -- tray --snapshot DIR --route appearance` with isolated saved 90% fixture | Pass: startup loads 90%, native alpha readback matches, WKWebView renders both themes at 16/20 px without overflow; saved preference remains 90%. |
| Windows native WebView2 composition | Not tested on this Mac. A matching Win32 alpha receipt alone would not prove WebView2 pixels. |
| Claude route cross-check (A1) | The [service-spawned Claude reviewer](https://github.com/taskqorg/taskq/issues/1812#issuecomment-6491722381) accepted A1: NSWindow alpha confirmed; Windows layered alpha remains unproved. |

The six [model screenshots](model/) use the actual bundled browser UI and CSS composition over an invented black/white checkerboard. They show Off, 95% and 90% in both themes, plus representative project/answer/resource body text. They are layout/color instruments, not native desktop-compositor evidence. The two WKWebView captures ([dark 20 px](wk-dark-20.png), [light 20 px](wk-light-20.png)) show the actual native web renderer at saved 90%; the capture API omits native alpha. All eight examples were visually inspected: labels, controls and final actions remain visible without horizontal overflow. Native composited screenshots and pixel contrast remain unknown; the color-pair test does not qualify antialiasing or every possible existing component state (R12).

Windows alpha application preserves the prior color-key attributes and verifies the complete native receipt; that correction has code-inspection evidence only on this Mac, with no Windows failure/pass reproduction. Startup at Off leaves native attributes untouched. Off requests full opacity again; Windows compositor behavior and visual rollback remain unverified. No installation, running service, capacity, permissions or palette of the tray icon is changed by this task. Publication still requires green merge-result `merge_group` CI (§ 7 / R12).

## Requeue continuation

The owner selected [Requeue + PM captures](https://github.com/taskqorg/taskq/issues/1812#issuecomment-6491759956). The worker rebases and refreshes the screenshot manifest; the PM supplies macOS compositor captures and a Windows WebView2 alpha run here. This answer does not waive A3.

Before: PR #1815 was conflicting, and `git rebase origin/main` reproduced the conflict in `docs/shots/manifest.json`. The rebase onto `baca245eb3769da59e14f7923cef2579179f8691` completed after resolving that generated manifest. Source behavior, range guards, existing assertions and the approved palette exception remain unchanged.

The first post-rebase canonical run found a real R8 regression: the combined root contract was 61,454 bytes, 14 bytes above 60 KiB. Its unchanged trace/size guard failed. The added root pointer now uses one linked M1.1 heading: 61,440 bytes and 299 lines. `cargo test --locked --bin taskq every_table_row_has_a_golden_trace` now passes. No existing rule text or guarding assertion was changed.

`sh docs/shots/update.sh` rebuilt the release and refreshed the source manifest from `b3ffc37786a59d87b65324c6e9742303669a4d49`. The six model frames were regenerated from that revision's UI bundle; all six, the native project page and the tray icon were visually inspected. Controls, body text and footer remain visible; these model frames still do not prove A3. All 25 Node component files, the opacity browser instrument, screenshot-manifest checks and release tag/exact-source checks passed after the rebase.

Final source check at `a35ec665`: `python3 scripts/check.py --base baca245eb3769da59e14f7923cef2579179f8691` remains FAIL/sandbox-blocked, not PASS: 576 Rust tests passed, 18 host-policy/lifecycle tests failed and 33 were ignored; 141.832 s on this warm-cache macOS worker. The failure set matches the prior attempt's 18 worker-context failures, including `a worker cannot install the host service`, `a worker cannot change host policy` and `a worker cannot restart the host service or request Stop`. R8's trace/size check and both opacity tests now pass. Permissions and test assertions were retained; merge-result CI must qualify those host checks. Offline `TASKQ_SELFTEST_OFFLINE=1 cargo run --locked -- selftest` passed 99 golden traces, formats and isolated telemetry; it did not read the board. The final evidence-only update changes no tested source or dependency.

A3 remains incomplete until the PM records native Off/95%/90% composition over a busy background in both themes, body-text contrast at 90%, and Windows WebView2/Off restoration results. Use `docs/shots/fixture.sh` with an isolated installation and `taskq tray --snapshot DIR --route opacity`; ordinary webview images and native alpha readback alone are insufficient. Keep the review set within 10 screens; no TCC prompt or permission change is authorized by this continuation.

### Latest integration candidate

Main moved to `47297c724e7295202178c2b3840f1832505eb29e` (#1814) during validation. A second rebase resolved the generated icon/manifest conflicts and preserved that Conversations change. Its extra root text required shortening only this task's added menu pointer and removing blank separators: the final contract is 61,440 bytes / 293 lines, with every existing nonblank rule and guarding assertion retained. The rule gate reports no changed rule or rule-citing test.

`sh docs/shots/update.sh` regenerated native project/icon captures and source hashes from `f1c9e387eae8686570b9a86ce9512e0b682cb72e`; the actual manifest freshness check passes. All 25 component files and the opacity browser instrument pass on the latest bundle. All six regenerated Off/95%/90% model frames were visually inspected; controls, body text and footer remain visible. Their composition remains a model, not A3 evidence.

`python3 scripts/check.py --base 47297c724e7295202178c2b3840f1832505eb29e` remains FAIL/sandbox-blocked: 579 passed, the same 18 worker-context host-policy/lifecycle failures, 33 ignored; 232.684 s on this macOS worker. The unchanged R8 trace/size guard and both opacity tests pass. Offline selftest again passes 99 traces, formats and private telemetry without reading the board. The final commit records these observations and the already-tested root formatting/assets; it changes no implementation or dependency. Native A3 and merge-result CI remain open.
