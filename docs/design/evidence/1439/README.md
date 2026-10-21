# Shared tray view-state evidence

Task [#1439](https://github.com/taskqorg/taskq/issues/1439), contract §4 and
Menu M8/UI02, implemented under R8/R12/R13. This fixture is invented data.

## Before and after

At baseline `dc3b9a8c`, `node tests/tray-view-state.browser.cjs` failed:
`navigation must reach the shared native settings adapter`. Route/filter/scroll
existed only in the controller, while native Settings persisted appearance/window
preferences. The same browser instrument now passes with a fresh page at both
`old` and `new` snapshot versions. It also passes service-stop observation,
Back, expanded Service group, selected project, removed project/route fallback,
live project removal and visible save failure.

The instrument substitutes only IPC delivery and the release-version observation.
`tray::tests::pin_and_position_per_display_survive_restart` exercises the real
Settings load/set/atomic-save/load boundary and retains every appearance/window,
notification cursor, off-screen position and size-clamp assertion. It additionally
checks navigation, unsupported schema and oversized input. This is not a live
installed-service restart or a real two-release update qualification.

## Browser review

Chromium headless shell, actual bundled tray, dark 16 px, 360×600 CSS px:

- [Restored older release, top](restored-old-top.png) and [kept scroll](restored-old.png).
- [Restored newer release, top](restored-new-top.png) and [kept scroll](restored-new.png).

The images were opened and visually inspected: the Doing capsule is selected,
18 matching rows are available, the header stays visible, and the scrolled image
retains the reading position. Browser rendering does not qualify WKWebView or
WebView2. `tray-answer.browser.cjs` retains draft/caret/focus, same-ID receipt,
unchanged-refresh/no-DOM-mutation and Back protections in both themes at 16/20 px.

The old navigation unit fixture used positional disclosure booleans; it now uses
stable keys and retains its original scroll/Back/filter assertions. The filter
instrument previously rejected every settings write; it now permits only the
shared navigation-only Settings message and still refuses every queue/focus write.
No assertions about queue authority, receipts or native sizing were removed.

## Native snapshot check

`sh docs/shots/update.sh` completed using the locally signed release binary at
source `3df748bc`. The isolated WKWebView Project and Icon captures were inspected;
the README Project image is byte-identical to the prior image. The manifest records
the new shared state module and current source blobs. This checks rendered native
regression only, not installed service/update restoration.

## Remaining qualification

`python3 scripts/check.py --base dc3b9a8c` ran the full Rust gate in 34.691 s:
430 passed, 18 failed, 34 ignored under the worker context.
Host operations report `a worker cannot change host policy`, `a worker cannot
install the host service` or `a worker cannot restart the host service or request
Stop`; downstream fixtures fail their expected saved/receipt assertions. These
host checks are sandbox-blocked, not PASS. Worker authority was not removed.
GitHub merge-result CI must qualify the full affected OS matrix before publication.
Installed service/update behavior and Windows native rendering remain unverified.
