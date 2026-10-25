# TaskQ #1700: View window native captures

Native macOS captures (WKWebView snapshot, macOS 27.0.1 arm64) of the View window at its default 360×600 size with eight projects, made by [`capture.sh`](capture.sh) through `taskq tray --snapshot --route views`. The fixture view `Review work` has saved state Doing and working states Doing and Review, so Save, Reset and Delete are live.

| Theme | Size | Top: Saved views, Projects, footer | States: Review choice |
|---|---|---|---|
| Dark | 16 px | [top](popover-dark-views-top-16.png) | [states](popover-dark-views-states-16.png) |
| Dark | 20 px | [top](popover-dark-views-top-20.png) | [states](popover-dark-views-states-20.png) |
| Light | 16 px | [top](popover-light-views-top-16.png) | [states](popover-light-views-states-16.png) |
| Light | 20 px | [top](popover-light-views-top-20.png) | [states](popover-light-views-states-20.png) |

What the captures show:

- Close stays in the header; Display, Save, Reset and Done stay in the fixed footer at default size with eight projects.
- Saved views, Projects and States share one content scroll; states keep the fixed order.
- The Review glyph draws the Review token: #8250DF light, #BCA3EB dark.

Limitation: these are WKWebView snapshots of the window surface, not screen captures; Screen Recording is not granted to the worker, so window decorations and placement on the desktop are not shown.
