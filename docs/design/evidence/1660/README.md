# Window chrome · #1660

Candidate for [#1660](https://github.com/taskqorg/taskq/issues/1660): separate Settings window, stable header, Home, one Conversations entry. Source: #1657 review-handoff R07–R09, R21–R26.

Browser checks run the bundled `build/web/tray.html` and `build/web/workspace.html` in the Chromium headless shell (`tests/browser.cjs`) with invented data and a stubbed IPC. They do not open native windows. Native items need the Windows and macOS tray and are reported separately.

## Ledger

| R | Source/browser | Native | Evidence |
|---|---|---|---|
| R07 stable header row | Pass | Not tested | `window-chrome.browser.cjs`: Home, Conversations, Settings, Keep open, Always on top, Close share one row at 320, 360 and 800 px; the project page keeps each icon's right offset; Back and a long title take row 2 with whole-word wrapping |
| R08 Home | Pass | Not tested | One Home activation returns to Tasks; Back then returns one page; `tray-navigation.test.cjs` keeps scroll and disclosures across Back |
| R09 one Conversations entry | Pass | Not tested | One header entry; badge shows the count `3`, opaque fill and WCAG AA contrast with and without the injected palette, both themes; with events it opens `messages`, with none the last section |
| R21 Auto growth and explicit choices | Pass | Not tested | `cargo test --locked window_anchor_preserves_edge_and_clamps_to_work_area`: Auto follows the edge and icon; explicit Down and Up survive placement |
| R22 Settings window | Pass | Not tested | The menu sends `settings-open` and keeps its dashboard; the Settings surface has its own Home, Back, Always on top, Enlarge and Close and sends no menu `size` or navigation writes. Native open/raise, hide on close, kept section and drafts, and no OS title bar are not run |
| R23 Close and Keep open on drag | Pass for Close | Not tested | Close is last in the first row at every width. First real drag turning on Keep open and the remembered mode are native |
| R24 Always on top | Pass | Not tested | Always on top sits in the action row with the filled-front squares icon (`ui-icons.json` `always-on-top`); labels `Keep menu open`, `Always on top` and the task row's `Pin task` differ. Native z-order is not run |
| R25 menu without Enlarge, grip | Pass | Not tested | No menu Enlarge; footer is `TaskQ v<version>` and the grip `Resize window with arrow keys`. Pointer and keyboard resize on the native window are not run |
| R26 Settings and Conversations Enlarge | Pass | Not tested | Settings and Conversations headers both show `Enlarge window`; native maximize is not run |

Native items not run here: drag, Keep open on first drag, z-order, hide, raise, maximize and the absence of an OS title bar. They stay Not tested until a tray run on Windows and macOS.

## Screens

Eight browser captures at CSS size, written by `CHROME_EVIDENCE=docs/design/evidence/1660 node tests/window-chrome.browser.cjs`.

| File | Shows | SHA-256 |
|---|---|---|
| [menu-home-320-dark.png](menu-home-320-dark.png) | Home at 320 px with the Conversations count | `c990592f12a5f09f287649b43efed27b573b2aa86203890a3024ce17ab057001` |
| [menu-home-320-light.png](menu-home-320-light.png) | Same, light | `27f8fd24559b2e31ec399b7372a7d8042e1add9d7baa117e799fd9d4343804ec` |
| [menu-project-320-dark.png](menu-project-320-dark.png) | Long project title in row 2 at 320 px | `e5891715afd9c9c276100329728c1376707ce0f29b247ce942c719f573a8b49b` |
| [menu-project-320-light.png](menu-project-320-light.png) | Same, light | `51f89cbf25729f4a252449887a46ccb77dced838cbb38939d4ddf165ceed4f42` |
| [menu-project-360-dark.png](menu-project-360-dark.png) | Long project title at 360 px | `83364dc1b1abe0804bf6e4dfd9c007267ebc9965cdec4d4422671003872733d9` |
| [menu-project-360-light.png](menu-project-360-light.png) | Same, light | `f373db11b778c2c6774bc030af149662a9885a9f7f723e29eed17108febb6566` |
| [settings-window-360-dark.png](settings-window-360-dark.png) | Settings window surface, Window section | `c8a1f58159e0a7ae25032e0c6d25799d3265d91be98d2f2c5abf3b17e53e48fd` |
| [settings-window-360-light.png](settings-window-360-light.png) | Same, light | `b52bacba7dd5ba428ad5d0836ea9d35e8d0e9f126f974c9b9e6b5360ce077827` |
