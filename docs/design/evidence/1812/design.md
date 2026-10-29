# Whole-window transparency: design checkpoint

Design for [#1812](https://github.com/taskqorg/taskq/issues/1812). The [owner answer](https://github.com/taskqorg/taskq/issues/1812#issuecomment-6490519452) approves stronger translucent text. R13 is applied in the root Product Menu opacity decision and M1.1; existing Off behavior and guarding assertions are retained.

## Current implementation

`src/tray.rs` creates an undecorated, transparent Tao window and transparent Wry webview for the menu. `src/task-workspace.css` paints the menu surface and its custom chrome. `Settings` loads, validates and saves local appearance preferences in `tray.json`; Settings forwards appearance actions to the menu. The menu's routes share one native window. Settings, View and Conversations have separate native windows.

There is no opacity preference, slider or native alpha application. Implementation would reach the root Product contract, M1 Appearance, the preference validator/save/readback, Appearance controls and IPC, native menu creation/application, and affected component/native snapshot checks. It must preserve unknown preference keys, navigation, pinned placement, icon rendering and separate windows.

## Native routes

| Route | What the user sees | Suitability |
|---|---|---|
| macOS `NSWindow.alphaValue` | The complete composited window, including text and custom chrome, fades uniformly. | Recommended for the requested whole-window effect; the existing `objc2-app-kit` dependency exposes `NSWindow`. |
| macOS `NSVisualEffectView` / vibrancy | A system material blends the backdrop and can modify foreground appearance. | A background/material effect, with additional native view integration; it does not implement uniform opacity. |
| Windows `WS_EX_LAYERED` + `SetLayeredWindowAttributes(LWA_ALPHA)` | Constant window alpha, in 255 steps, including foreground content. | Recommended Windows counterpart, using the existing `windows` dependency. Compatibility with the actual WebView2 child rendering still needs a native check; API availability is not that proof. |
| WebView2 transparent background | Transparent web pixels reveal the host; opaque page content remains opaque. | Already used by the menu. `DefaultBackgroundColor` accepts only alpha 0 or 255; it is not a continuous whole-window opacity control. |
| Windows Mica / Acrylic | Mica incorporates wallpaper; Acrylic provides a translucent backdrop. Foreground content remains separate. | These materials change the background, not the complete window's alpha, and have system-dependent fallbacks. |

Choose native constant alpha on each OS if the whole-window requirement remains. Use the same persisted percentage on both; validate it before native calls and report application/save failures rather than claiming success. Off restores alpha 1.0 / 255. Apply only to the menu window, independent of its route; retain the other windows and tray icon at their existing opacity. A Claude reviewer must cross-check this recommendation; that review has not happened.

Sources: Apple [window alpha](https://developer.apple.com/documentation/appkit/nswindow/alphavalue) and [visual effects](https://developer.apple.com/documentation/appkit/nsvisualeffectview); Microsoft [layered-window alpha](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes), [WebView2 background](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2) and [materials](https://learn.microsoft.com/en-us/windows/apps/develop/ui/materials).

## Contrast constraint before implementation

The [approved palette](../../visual-style.md#palette) predates this feature. Whole-window alpha fades text as well as the surface. A range that protects primary body text does not automatically protect secondary or semantic text.

Development calculation: composite each source channel as `alpha * source + (1 - alpha) * backdrop`, then use the [WCAG relative-luminance contrast formula](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html). A busy background may be white beneath a glyph and black beneath its neighboring light-theme surface; reverse those extremes for the dark theme. These are conservative color-pair calculations, not native screenshot measurements. The following values use exact floating-point alpha; Windows byte quantization must also be tested.

| Foreground / surface | Off | 90% opacity | 95% opacity |
|---|---:|---:|---:|
| Light primary `#243039` / `#FAFBFC` | 13.02:1 | 7.60:1 | 10.01:1 |
| Light secondary `#536171` / `#FAFBFC` | 6.11:1 | 3.85:1 | 4.85:1 |
| Light Work `#087F5B` / `#FAFBFC` | 4.83:1 | 3.22:1 | 3.96:1 |
| Dark failure `#FF6378` / `#252B32` | 4.97:1 | 2.98:1 | 3.87:1 |

The current unfilled Work and failure text pairs pass 4.5:1 at Off and fail at both proposed translucent values. No implementation fix or after-pass is claimed. A useful broad slider range therefore needs an owner choice about the palette or effect; choosing a barely translucent range solely to pass these pairs would hide that product tradeoff. This calculation does not qualify every existing text/surface pair.

## Owner decision

The owner selected stronger translucent text (option 2 below). The adopted range is 90–100% in 1% steps, with Off at 100%. Primary text is #12181C / #FFFFFF; semantic and secondary tokens mix with 50% black / 90% white in light/dark mode. This is limited to the translucent menu. The following alternatives explain the original decision; they are not open choices.

1. Keep the approved palette and uniform alpha. The range must stay close to opaque; determine its exact minimum against all affected text surfaces and Windows quantization before adopting it.
2. Permit stronger text colors while translucent. Keep uniform alpha and determine a wider AA-safe range; this changes an approved visual decision. Recommended because it preserves the requested whole-window effect without making its range practically invisible.
3. Make only the background translucent. Preserve foreground text contrast, but explicitly change the whole-window requirement; do not silently substitute vibrancy or a material.

Verification: persistence/route coverage, Off/midpoint/minimum over an invented busy background in both themes, at most 10 screens. `tray --snapshot DIR --route opacity` produces six explicitly selected cases; its webview captures remain separate from native screen captures. Native composition and text contrast must be checked on macOS and Windows. Ordinary WKWebView/WebView2 page snapshots omit native window alpha and cannot alone prove A3. Candidate checks and platform limitations are recorded in [verification](verification.md); source checks do not qualify a missing platform (R12).
