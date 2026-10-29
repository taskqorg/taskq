# Background-only transparency verification · #1813

The owner selected [AA-safe backing](https://github.com/taskqorg/taskq/issues/1813#issuecomment-6493358099). [Design comparison](design.md) covers blur, backing, shadow and native materials per OS. R13 is applied through the root contract link, new M1.2 and the linked visual treatment; R1–R13 and M1.1 behavior are preserved.

UI source candidate: `c32296d1575721e05672371ebb16f252f43519f9`, rebased on `b8c05120f9385e8a7753a9a780cff0a984394a50`. The final UI source revision and native capture are recorded below. Environment: macOS 27.0.1 arm64, Node 22.18.0, Rust/Cargo 1.98.1, Playwright Chromium headless shell through `tests/browser.cjs`. Fixtures are invented; they perform no live task or PM operation.

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

## Remaining acceptance

- A1 Claude reviewer cross-check: pending the service-spawned reviewer (R3). This worker does not self-certify that review.
- macOS WKWebView snapshots and README manifest refresh: final capture follows the native range-cue correction.
- Native desktop composition over actual wallpapers, native interactive mode switching and Windows WebView2: Not tested. A browser result, WKWebView page snapshot or native alpha API readback does not establish those claims (R12).
- Owner visual acceptance and green merge-result CI: pending. No installed service or application was replaced.
