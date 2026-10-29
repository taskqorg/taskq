# Native composition evidence · #1812

PM capture for A3, taken on the release build of PR head `45be413d`.

## Method

- Isolated fixture: `TASKQ_VERSION=4.6.40 sh docs/shots/fixture.sh DIR`, then `TASKQ_INSTALL=DIR/install taskq tray --snapshot OUT --route opacity` (exit 0, all six cases).
- The menu window was composited by the macOS window server over a busy desktop backdrop and captured as on-screen region frames, not webview renders.
- Contrast: every glyph-core pixel of a text line against every background pixel inside its band, WCAG relative luminance. The table gives the minimum ratio.

## Screens

| File | Case |
|---|---|
| [mac-dark-100.png](mac-dark-100.png) | Dark, Off (100%) |
| [mac-dark-90.png](mac-dark-90.png) | Dark, 90% over the busy backdrop |
| [mac-light-90.png](mac-light-90.png) | Light, 90% over the busy backdrop |
| [mac-light-off-after-90.png](mac-light-off-after-90.png) | Off restored after 90%: opaque, no residual translucency |

## Body-text contrast at 90% (AA needs 4.5:1)

| Theme | Line | Minimum ratio | Off |
|---|---|---|---|
| Dark | Secondary "Theme follows system appearance." | 8.40 | 8.23 |
| Dark | Body "Text size · 16 px" | 8.86 | 12.69 |
| Dark | Body "Menu opacity" | 10.91 | 12.69 |
| Light | Secondary "Theme follows system appearance." | 10.75 | 6.11 |
| Light | Body "Text size · 16 px" | 14.15 | 13.05 |
| Light | Body "Menu opacity" | 14.15 | 13.05 |

At 95%: dark body 12.47, light body 15.65. Every measured line passes AA.

## Results

| Check | Result |
|---|---|
| macOS composition Off / 95% / 90%, light and dark | Pass |
| macOS body-text contrast at 90% | Pass (minimum 8.40:1) |
| macOS Off restoration after 90% | Pass |
| Windows WebView2 composition and Off restoration | Not tested: the release build compiles on the Windows host, but its snapshot adapter reports native composition capture unavailable and no interactive desktop capture was available to the PM |
