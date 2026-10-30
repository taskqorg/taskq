# Bell warning overflow regression

This fixes #1994 without changing a product decision (R13). The notification warning now stays inside its existing bell target: `bottom:0;right:0` replaces the two negative 3 px offsets. Numeric badges, accessible names, warning rendering and `tray.measure` are unchanged. No overflow exemption or threshold change was added.

## Failure before the fix (A1)

Source `554552daf48d5b413918d54a8a6d0f88efba4020`, executable SHA-256 `6112ec8f3c230d963150608764e85ebde98e67aac8d22d36ecc2d9f57b54d8f5`. The original #1933 debug executable was reused read-only with its x64 WebView2 loader directory on the child PATH. Earlier launches without that directory failed before rendering; they are not overflow evidence.

The unchanged `docs/design/tray-fixture.sh <fixture> 0` creates three disposable projects, including partial/stale observations and six loaded unread notifications. The snapshot child selects only `<fixture>/install`, disables incident reporting and drops inherited task, attempt, session and board-token environment bindings. No service is started or stopped, and no production grant, view or answer is changed.

```text
taskq tray --snapshot <output> --route home,project,settings,appearance,hosts,cadence,notification-settings,agents,projects,delivery,services,recovery,about
```

The command produced 50 native PNGs and exited **1**. All 50 diagnostics report `button 6 by 3 px`; [before.txt](before.txt) preserves them without machine paths. The existing snapshot instrument renders each listed route in dark/light at 16/20 px, except Project, which it renders at 16 px only. This task does not expand that instrument or claim Project at 20 px.

The added warning-boundary assertion in `node tests/notifications.browser.cjs` also failed before the CSS fix: `Incomplete-history warning fits inside the bell target`.

## Pass after the fix (A2)

The same command and fixture generator completed **50 native WebView2 captures, exit 0, with no overflow diagnostics**, in 44.859 s. Windows text size remained 140% before and after. Settings and the other two-font routes passed dark/light at 16/20 px. [after.txt](after.txt) records the immutable implementation commit `0eb09f234faef6ff99bfc32d1215e64909dff671` and binary SHA-256 `6a7cbdfa28afbcece06808a3a0c6ab09817a4e99054b64958eb4ab4bbe223589`. The subsequent commit adds evidence and extends the same browser check for the #1992 steer; product implementation bytes are unchanged.

The debug build passed in 7m 21s with 25 compiler warnings. These warnings were not changed as part of this CSS regression fix. Both representative after captures were inspected at their native dimensions: the six-count badge and separate warning remain visible, and the warning sits within its bell target.

## Retained protection (A3)

The existing notifications browser instrument retains navigation, exact counts, grouped events, failed receipts, bulk-read races, clearing and question preservation. Its four theme/font Chromium cells now also exercise Windows and macOS platform modes on both Home and Settings, requiring the partial-history warning to fit inside a bell target at least 32 by 32 CSS px, both with 105 unread and with zero loaded unread. The accessible count remains exact, `99+` remains only the visual cap, and an unknown/partial history still shows a separate warning.

Each case invokes the actual `taskq.measure` overflow gate and requires no warning overflow. An intentionally width-constrained label then must produce a button-overflow diagnostic. This guards real clipping rather than excluding the bell or suppressing the gate. The same browser check passes after the CSS fix in dark/light at 16/20 px and 320 px width.

## Native environment and limits (R12)

Windows registry: 26H2, build 26300.9711; `TextScaleFactor=140` throughout both native runs. One screen: 2560 by 1440, usable area 2560 by 1392. Display DPI is not independently qualified. Rust/Cargo 1.99.0, stable Windows GNU; Node v24.5.0; candidate build uses four jobs. This is debug-build WebView2 snapshot evidence, not an installation, release qualification, owner visual approval or proof of native clicks. The fresh inbox note 6535752046 consolidates the same #1992 macOS defect here. Native macOS proof remains explicitly unverified until #1932 resumes; Windows native evidence and the shared browser snapshot check are recorded here. Broader Windows native qualification remains #1933.

The complete capture sets and raw logs stay in the disposable run directory. The following four unmodified captures form the review set; they show the same bell count and separate warning before and after.

| Scene | Before | After |
|---|---|---|
| Dark Home, 16 px | [capture](before-popover-dark-home-16.png) | [capture](after-popover-dark-home-16.png) |
| Light Settings, 20 px | [capture](before-popover-light-settings-20.png) | [capture](after-popover-light-settings-20.png) |

## Capture checksums

| Capture | SHA-256 |
|---|---|
| after-popover-dark-home-16.png | `3ed389a93b12d858986d5d2eeda66f223a0b0cda139d136fce69fa90faef7c1a` |
| after-popover-light-settings-20.png | `65672d98b0e0f77d8532bd641a1357299107c65f11ed13d13c96eb2627e768f8` |
| before-popover-dark-home-16.png | `5a4d237c9f3be2e28e4945b37d894e93a294b6f67e187d4f7e303340a7f25898` |
| before-popover-light-settings-20.png | `50361b88ba18f80d16399b30ff0adab157b3f0145c8a7dccd2169059129d6ff5` |

## Canonical gate isolation

The first `python scripts/check.py --base d91c43438a15f333b05d196e885937e681ecfb11` selected the full suite and failed after 265.172 s: 744 unit checks passed, two failed, 39 were ignored. The failures were `host::tests::local_capacity_crashes_never_replay_an_uncertain_write` (`committed` instead of `intent-saved`) and `host::tests::power_loss_zeroed_records_are_set_aside_only_without_an_owner` (two saved records instead of one).

Both fixtures use a name plus process ID. Readback found their reused-ID directories already existed from October 27, with an old `committed` acknowledgement and a previously saved zeroed record. No old fixture was deleted. Both unchanged tests passed with `cargo test --locked --bin taskq <exact-name> -- --exact` under a fresh disposable `TEMP`/`TMP`, using the canonical runner's isolated installation and environment helper. The full gate is rerun with the same fresh-TEMP isolation; this corrects the observed environmental contamination without changing a host assertion or production source. Initial failures remain recorded under R12.

The fresh-TEMP canonical gate passed in **195.063 s / 900 s**, with 746 unit checks passed (39 ignored), 19 integration checks passed (one ignored), offline selftest, all selected Node/component and artifact-script checks, and the expanded notifications browser instrument. The offline selftest explicitly leaves board and tool probes unknown; it makes no live-board claim. There were no retries within either gate invocation. Merge-result `merge_group` CI remains a publication gate under R12 and ?7; it was not awaited by this worker.
