# Background-only transparency · #1813

Stage 1 comparison; the owner selected AA-safe backing in [answer 1](https://github.com/taskqorg/taskq/issues/1813#issuecomment-6493358099). Task: [#1813](https://github.com/taskqorg/taskq/issues/1813). Baseline: `6bf0a4aeca2ec679c7c3523c4f6cb4abfa00a34a`, including the delivered whole-window mode from #1812. R13 is applied through Product, M1.2 and the linked backing treatment; R12 keeps renderer and platform evidence separate.

## Existing boundary

M1.1 uses native constant window alpha and a stronger translucent text palette. The menu already has a transparent Wry window/webview, while CSS paints its surfaces. Appearance, settings persistence and native readback are in `src/tray.rs` and `src/tray-app.js`; menu styles are shared with the separate Settings, View and Conversations windows. Those windows and the tray icon must retain their current behavior.

The second mode must restore native alpha to 100% before exposing background-only rendering. Setting `opacity` on the menu, a content ancestor or a semantic element would fade the foreground and fail A2/A3. Change surface, chrome, border and divider color alpha instead. Text, icons, buttons and badges need full element and ancestor opacity, including disabled controls; their unavailable state still needs an accessible explanation and a distinct opaque presentation. Do not remove the existing invisible file-input hit target when checking rendered semantic content.

## Methods compared

| Method | macOS / WKWebView | Windows / WebView2 | Decision |
|---|---|---|---|
| Backdrop blur | Newer WebKit can filter behind a transparent webview. A CSS feature check alone does not prove desktop composition on the supported host. | Filtering the web document is not proof of filtering the desktop through WebView2. Native desktop blur needs a separate host composition path. | Do not rely on blur for contrast. A uniformly adverse backdrop remains adverse after blur. |
| Scrim or backing plate | CSS surface colors can paint a bounded, theme-colored backing behind content while foreground alpha stays 1. | Reuse the already transparent webview; paint the backing in web content. The host background API itself only accepts alpha 0 or 255. | Recommended: one shared CSS treatment, no new dependency or native material adapter, measurable contrast and predictable fallback. Prefer content-area backing to a new plate for every glyph. |
| Text shadow | CSS can add a halo around glyphs. | Same web treatment. | Insufficient as the only protection: it does not protect icon/control boundaries, and the glyph/background pair can still fail contrast. It also changes the accepted typography. |
| Native vibrancy / material | `NSVisualEffectView` supports behind-window blending and foreground vibrancy; appearance depends on material and system settings. Web foreground integration is additional work. | Acrylic provides desktop translucency; Mica is opaque and incorporates wallpaper. System preferences and hardware can produce solid fallbacks. Neither is a drop-in Wry CSS implementation. | Defer: two native paths, variable appearance and more qualification. Neither is necessary for background-only color alpha. |

Sources: Apple [behind-window blending](https://developer.apple.com/documentation/appkit/nsvisualeffectview/blendingmode-swift.enum/behindwindow) and [visual effects](https://developer.apple.com/documentation/appkit/nsvisualeffectview); WebKit [Safari 26 webview changes](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/); Microsoft [WebView2 background](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2) and [materials](https://learn.microsoft.com/en-us/windows/apps/develop/ui/materials).

## Contrast policy

The task does not specify the new background range, default backing strength or a minimum backing strength. A2 calls the backing optional; A3 requires AA in the minimum-opacity shots. Shared D7 also requires text contrast of at least 4.5:1. Full foreground opacity alone cannot guarantee contrast when the background can disappear and the backing can be disabled. Blur alone cannot guarantee it either.

Read-only reproduction: composite the existing surface as `a * surface + (1 - a) * wallpaper`, keep text fully opaque, then apply the W3C relative-luminance formula. These calculations are development evidence, not screenshots or native measurements.

| Existing primary text / surface | 0% surface opacity | 50% surface opacity | 90% surface opacity | 100% surface opacity |
|---|---:|---:|---:|---:|
| Light `#243039` / `#FAFBFC`, black wallpaper | 1.56:1 | 3.30:1 | 10.39:1 | 13.02:1 |
| Dark `#EEF2F5` / `#252B32`, white wallpaper | 1.13:1 | 2.67:1 | 9.27:1 | 12.69:1 |

Both minimum cases fail 4.5:1 despite foreground alpha 1. Semantic/secondary colors and selected/control surfaces need their own checks; these primary pairs do not qualify them. No fix or after-pass is claimed. Raising the new minimum until the effect is barely visible would conceal the product tradeoff rather than resolve it.

Adopted owner choice: allow a transparent decorative background, but retain an AA-safe minimum backing behind readable content and chrome controls. The strength control may add backing; Off is available only where the background itself supplies the required contrast. The implementation uses a 98% Surface backing floor, with opaque Surface beneath controls and selected/hover content, preserving the accepted palette unless the owner explicitly approves another change. Cost: content regions cannot become arbitrarily transparent.

Rejected alternative: permit unrestricted low background opacity with backing Off, and explicitly approve conditional AA plus the D7 exception. Cost: some wallpapers make content unreadable; minimum-opacity screenshots with backing enabled would prove only that configuration, not the unsafe settings.

## Implementation and verification

1. Record the answer in the root contract and its linked menu/visual specification before code (R13). Keep M1.1 and its existing range, persistence and native failure protection.
2. Add the mutually exclusive background-only mode beside the whole-window mode, independent saved background opacity and backing strength, validated IPC and startup/readback. Switching modes must not leave native alpha below 100% in background-only mode; rollback and save errors remain visible.
3. Reuse the current Appearance controls and shared CSS. Preserve palette, geometry, navigation, drafts, disabled semantics and separate-window opacity. Check normal, selected, disabled, error, popover and shared project/answer/resource content.
4. Extend existing opacity browser/preference checks at their affected boundary. Add a rendered-content check for foreground and ancestor opacity, with explicit handling of invisible hit targets. Retain whole-window-mode checks. Record failure before and pass after the implementation.
5. Capture the minimum background opacity over busy light and dark backdrops, at most 10 screens total. Measure ordinary text at 4.5:1 and meaningful icons/control state cues at 3:1; decorative dividers and inactive content have the W3C exceptions. If the chosen policy promises more than the captured configurations, check its boundary combinations too. Inspect the actual images. Browser composition is separate from native desktop composition on macOS and Windows; unavailable evidence stays Not tested.

Contrast sources: W3C [text minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) and [non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html). Review mechanics follow [UI acceptance](../../ui-acceptance.md).

## Checkpoint status

- A1 comparison and technical recommendation: drafted above. Service-spawned Claude reviewer cross-check: Not tested; no reviewer was launched by this worker (R3).
- A2 implementation: controls, independent settings, native alpha restoration and shared backing are implemented. See [verification](verification.md) for observed checks.
- A3 evidence and platform limits: [verification](verification.md).
- Existing R1–R13 and M1.1 behavior: unchanged. Existing settings assertions now include the new default fields; their validation and preservation protection is retained. No host policy, permission, service or installed-runtime change was applied.
