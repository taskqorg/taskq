# Background-only transparency verification · #1813

The owner selected [AA-safe backing](https://github.com/taskqorg/taskq/issues/1813#issuecomment-6493358099). [Design comparison](design.md) covers blur, backing, shadow and native materials per OS. R13 is applied through the root contract link, new M1.2 and the linked visual treatment; R1–R13 and M1.1 behavior are preserved.

Final UI source: `c802f8973cc8bedfb4eea952abe765c932b97714`, rebased on `93877cc0ced95a0858dc72d829ba51337f33df59`. The final UI source revision and native capture are recorded below. Environment: macOS 27.0.1 arm64, Node 22.18.0, Rust/Cargo 1.98.1, Playwright Chromium headless shell through `tests/browser.cjs`. Fixtures are invented; they perform no live task or PM operation.

## Failure before and protection after

- Before implementation, `node tests/tray-settings.test.cjs` failed with `Appearance must expose background-only transparency`. After: Pass. The same file retains its whole-window range/Off, sign-in, access, route and recovery assertions.
- Opaque primary text over an unbacked adverse wallpaper measured 1.56:1 light / 1.13:1 dark in the design calculation. The new mode uses native alpha 100%, background opacity 0–100% and a 98–100% content backing. Independent whole-window opacity remains 90–100%; switching back restores that saved value.
- The browser check resolves actual computed foreground and ancestor background colors, then composites both black and white outside pixels. It checks ordinary text at 4.5:1 and icons/control cues at 3:1. Inactive controls retain full opacity but use the W3C contrast exception. Shared project/answer/resource body styles are included. This is a conservative CSS color calculation over rendered elements, not a native screenshot pixel measurement.
- The alpha guard checks semantic elements and their ancestors, including disabled controls and SVG descendants. Injecting 50% opacity into a control, SVG or content ancestor must fail; removing the mutation must pass. It also refuses a partially transparent foreground color. These mutations demonstrate sensitivity without weakening the existing protection.

## Checks

| Boundary | Command / observation | Result |
|---|---|---|
| Appearance controls | `node tests/tray-settings.test.cjs` | Fail before; Pass after |
| Persistence and native-alpha selection | `cargo test --locked --bin taskq tray::tests::background_opacity` | Pass: legacy preference retained; both independent ranges reload; malformed IPC refused; native alpha selects 100% in background mode |
| Existing whole-window behavior | `cargo test --locked --bin taskq tray::tests::opacity` | Pass: both existing validation/reload and adverse-composition palette cases |
| Browser integration | `node tests/tray-background.browser.cjs` | Pass: IPC, mode/window isolation, minimum backing, disabled content, shared styles, narrow/large text and opacity mutation checks |
| Existing browser behavior | `node tests/tray-opacity.browser.cjs` | Pass: keyboard/Off IPC, strengthened whole-window palette, route adoption and opaque Settings |
| Shared assets | `node tests/project-blocks.test.cjs`; `node tests/task-workspace.test.cjs` | Pass: source-provenance manifests refreshed for intended changes; existing assertions retained |

## Canonical gate and remaining checks

`python3 scripts/check.py --base 93877cc0ced95a0858dc72d829ba51337f33df59` completed in 185.169 s on this Mac with a warm dependency cache. Result: **sandbox-blocked**, not Pass: 592 Rust tests passed, 18 failed in worker-bound host-policy/lifecycle fixtures, 33 were ignored. Observed refusals include `a worker cannot change host policy`, `a worker cannot install the host service`, `a worker cannot restart the host service or request Stop` and `a worker cannot dispatch host Stop`. The three opacity tests and the R8 trace/size check pass. No permission or worker-role override was applied. Per § 7, the acceptance gate remains merge-result CI; the local failure is not presented as a green gate.

Because the Rust failure stops `check.py` before its remaining commands, those checks ran separately: all 26 `tests/*.test.cjs` files, screenshot-manifest checks, release-tag/exact-source checks and both transparency browser instruments passed. The component run first caught the finding-composer manifest's transitive pin of the changed project-blocks provenance file; that intended dependency hash was refreshed and the same run passed. No guarding assertion was changed. Offline selftest in an isolated install passed 99 golden traces, formats and private telemetry capture; no board read.

## Visual review

Generate the six browser images with `TASKQ_BACKGROUND_SHOTS=docs/design/evidence/1813/browser node tests/tray-background.browser.cjs`. Four home captures use minimum background opacity (0%) and minimum backing (98%): both themes over invented busy light and dark wallpapers. Two Appearance captures show the controls. The wallpaper is inside the browser document, behind the actual bundled menu; these are browser composition evidence, not desktop composition proof.

The four home captures were inspected at actual size. Required text, state glyphs, counters, project actions and custom chrome are visible; no clipping or vanished control was observed. Appearance was inspected in both themes, including mode selection, disabled whole-window slider and separate background/backing sliders. Existing geometry is preserved; intentional M1.2 differences are the backing areas, opaque control fills and full-opacity disabled controls. Selection retains its outline. No new icon or typography system is introduced.

| Capture | Overall computed contrast lower bound | Full foreground/ancestor opacity |
|---|---:|---|
| [Light over light wallpaper](browser/light-light-wallpaper.png) | 4.62:1 | Pass |
| [Light over dark wallpaper](browser/light-dark-wallpaper.png) | 4.62:1 | Pass |
| [Dark over light wallpaper](browser/dark-light-wallpaper.png) | 4.69:1 | Pass |
| [Dark over dark wallpaper](browser/dark-dark-wallpaper.png) | 4.69:1 | Pass |

Ratios include both text and semantic SVG/control samples and use independently adverse backdrop pixels, rather than rounding a failing value up. The executable guard uses unrounded values. [Light Appearance](browser/light-appearance.png) and [dark Appearance](browser/dark-appearance.png) retain the same minimum settings.

## Native range-cue correction

WKWebView initially rendered the enabled light-theme range thumb white despite the existing `accent-color`. The preserved [before image](native/before-light-range.png) measures 1.08:1 against the adjacent backing, below the 3:1 control-cue requirement. The fix paints opaque Work-colored enabled thumbs, Secondary-colored disabled thumbs and Secondary tracks, preserving the accepted palette and disabled semantics.

Final WKWebView captures: [light](native/light-appearance-20.png) and [dark](native/dark-appearance-20.png). The same thumb-core sample now measures 4.83:1 light / 8.99:1 dark. All sampled thumb pixels have alpha 255. [Pixel values and coordinates](native/range-contrast.json) record source-over composition against the adverse wallpaper and the WCAG formula; [before provenance](native/before-capture.json) preserves the failing artifact. This qualifies these native range cues, not every native text or icon pixel. The affected review set contains nine images: six browser captures, two final WKWebView captures and one distinct failing native capture.

The first canonical attempt stopped before tests because the added Product link changed an unnamed existing rule. Product is now byte-for-byte unchanged from the base; the root preamble links the new M1.2 through the existing menu reference. The rule gate passes without weakening or renaming any existing rule or guarding assertion. The root remains within 300 lines / 60 KiB.

## Remaining acceptance

- A1 Claude reviewer cross-check: pending the service-spawned reviewer (R3). This worker does not self-certify that review.
- macOS WKWebView page snapshots and README manifest refresh: Pass. Both final 20 px Appearance captures were visually inspected; the saved mode/background/backing values were unchanged. [Capture identity and hashes](native/capture.json).
- Native desktop composition over actual wallpapers, native interactive mode switching and Windows WebView2: Not tested. A browser result, WKWebView page snapshot or native alpha API readback does not establish those claims (R12).
- Owner visual acceptance and green merge-result CI: pending. No installed service or application was replaced.
