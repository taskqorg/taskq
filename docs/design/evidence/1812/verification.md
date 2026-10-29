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
| `node tests/tray-opacity.browser.cjs` | Pass: keyboard range/Off IPC, route adoption, shared body tokens, visible refusal, opaque Settings and narrow/large-text layout. |
| `cargo run --locked -- tray --snapshot DIR --route opacity` with isolated fixture | Native macOS alpha readback matched all six Off/95%/90% cases. Composition capture is sandbox-blocked: `Screen Recording not allowed (TCC); no prompt requested`. Exit 1 is not a pass. |
| `cargo run --locked -- tray --snapshot DIR --route appearance` with isolated saved 90% fixture | Pass: startup loads 90%, native alpha readback matches, WKWebView renders both themes at 16/20 px without overflow; saved preference remains 90%. |
| Windows native WebView2 composition | Not tested on this Mac. A matching Win32 alpha receipt alone would not prove WebView2 pixels. |
| Claude route cross-check (A1) | Pending the service-spawned reviewer (§ 7); no substitute reviewer was launched. |

The six [model screenshots](model/) use the actual bundled browser UI and CSS composition over an invented black/white checkerboard. They show Off, 95% and 90% in both themes, plus representative project/answer/resource body text. They are layout/color instruments, not native desktop-compositor evidence. The two WKWebView captures ([dark 20 px](wk-dark-20.png), [light 20 px](wk-light-20.png)) show the actual native web renderer at saved 90%; the capture API omits native alpha. All eight examples were visually inspected: labels, controls and final actions remain visible without horizontal overflow. Native composited screenshots and pixel contrast remain unknown; the color-pair test does not qualify antialiasing or every possible existing component state (R12).

Windows alpha application preserves the prior color-key attributes and verifies the complete native receipt; that correction has code-inspection evidence only on this Mac, with no Windows failure/pass reproduction. Startup at Off leaves native attributes untouched. Off requests full opacity again; Windows compositor behavior and visual rollback remain unverified. No installation, running service, capacity, permissions or palette of the tray icon is changed by this task. Publication still requires green merge-result `merge_group` CI (§ 7 / R12).
