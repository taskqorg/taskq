# #2375 native Mac frame and resize qualification

This is evidence for the parent task #2363 and its draft PR #2371. It holds a cause, a correction patch as data, and raw native logs. No file under `src/` changes here, and nothing was installed.

## Builds under test

| Label | Source | Notes |
|---|---|---|
| `installed` | `4.6.77-261104-7` from `baf6b2827e3e9d22dd6e5b80da69e6b51512acae`, dev channel | The live tray, sampled read-only |
| `installed-src` | `026db2524d6d5d7b34452d4c8d0fbf17073df7b9` plus `instrument.patch` | Source of `4.6.77-261104-6`, live when this task started; `src/tray.rs`, `src/tray-app.js`, `Cargo.toml` and `Cargo.lock` are unchanged in `-7` |
| `parent` | `1b2371899d836213a0b4dce30196eb613b568333` plus `instrument.patch` | The frozen #2363 tree |
| `fix` | `parent` plus `correction.patch` | The proposed correction |

Every instrumented build ran in `tray --snapshot` from an isolated temporary install with invented fixture data, in the dev profile, one run at a time.

## Cause

The mismatch is specific to macOS and sits between two libraries:

- `wry` 0.57.0, `src/wkwebview/mod.rs:688`, calls `ns_window.setContentView(parent_view)`.
- `tao` 0.37.1, `src/platform_impl/macos/window.rs:735`, answers `inner_size()` from `NSView::frame(&self.ns_view)`. That is its own view, now detached from the window, so the answer stays at the build size of 640x800 physical pixels for the life of the window.

The `"size" | "resize"` handler in `src/tray.rs` then computes `frame = outer - inner`. That value grows with the window, so `max_h = work_h - frame` depends on the current window height:

```text
cap = H + I - O     (H work-area height, I stale inner height, O current outer height)
next inner = min(natural, cap)
```

A natural height above about (H + I - 32) / 2, roughly 645 points here, has no fixed point: each fit flips between two heights. `anchored_position` uses the same frame, so the position and bottom edge move too.

- At `installed-src` the page refits on every native `resize`, which closes an endless loop: 888 native resizes in 15 idle seconds, 40.68% main and 21.65% WebContent CPU.
- At `parent` the page refits only on a width change. The loop stops, but each fit still flips the height (`parent` bottoms 1164 and 1680). This masks the cause and does not remove it.
- Two more call sites read the stale value. `size-anchor` Fixed saves 320x400 whatever the window measures, and `native_header_drag` compares the pointer with the stale height.

Windows and Linux report a real inner size, so `client_size` leaves them on `inner_size`. Neither was run here.

## Correction

`correction.patch`, sha256 `b1272bc089b45b769dff62401853f73ef46bdfc11213ac132b028177121b9eb1`, applies cleanly to `1b2371899d836213a0b4dce30196eb613b568333` and needs no `Cargo.lock` change.

- `client_size` reads `contentRectForFrameRect(frame)` from the `NSWindow`. It replaces `inner_size` at the three call sites. This removes the cause, with no timer, debounce or suppressed request.
- `native_titlebar` adds an `NSVisualEffectView` with the titlebar material behind the standard window controls of the main window. The window stays transparent, so the body keeps its background-only opacity.
- `Cargo.toml` adds the `NSVisualEffectView` feature to `objc2-app-kit`.

## Bounded native sequence

`qualify.py` runs the same script for each build and growth anchor: rest 15 s, content (12, 3, 40, 3 tasks), wrapping (long titles at font sizes 18, 14, 20, 18), straddle (8 to 22 long tasks across the critical height), titlebar (title changes, hide and show, light and dark), and rest again after 15 s. Native bottoms are physical pixels at scale 2. Columns come from `runs/<build>-<anchor>/summary.json`.


### Anchor `top`

| Phase | Build | Native resizes | Moves | Size requests | Page resizes | Native bottoms (px) | Main % | WebContent % |
|---|---|---:|---:|---:|---:|---|---:|---:|
| rest | installed-src | 888 | 0 | 888 | 888 | 1122, 1722 | 40.68 | 21.65 |
| rest | parent | 0 | 0 | 0 | 0 | - | 0.2 | 0.02 |
| rest | fix | 0 | 0 | 0 | 0 | - | 0.21 | 0.02 |
| content | installed-src | 739 | 0 | 740 | 736 | 1122, 1722 | 39.79 | 26.78 |
| content | parent | 4 | 0 | 4 | 4 | 1164, 1680 | 1.33 | 0.71 |
| content | fix | 4 | 1 | 4 | 4 | 1776, 1912 | 1.18 | 0.65 |
| wrapping | installed-src | 520 | 0 | 523 | 519 | 1122, 1722 | 43.32 | 15.87 |
| wrapping | parent | 6 | 0 | 6 | 6 | 1164, 1680 | 1.34 | 0.75 |
| wrapping | fix | 3 | 0 | 6 | 3 | 1912 | 1.15 | 0.72 |
| straddle | installed-src | 658 | 0 | 660 | 652 | 1122, 1722 | 49.97 | 22.28 |
| straddle | parent | 8 | 0 | 8 | 8 | 1164, 1680 | 2.96 | 1.09 |
| straddle | fix | 0 | 0 | 8 | 0 | - | 2.15 | 0.83 |
| titlebar | installed-src | 88 | 0 | 87 | 85 | 1122, 1722 | 8.85 | 4.27 |
| titlebar | parent | 2 | 0 | 2 | 2 | 1164, 1680 | 1.27 | 0.6 |
| titlebar | fix | 0 | 0 | 2 | 0 | - | 1.14 | 0.66 |
| rest-after | installed-src | 1 | 0 | 1 | 0 | 1722 | 0.18 | 0.19 |
| rest-after | parent | 1 | 0 | 1 | 1 | 1680 | 0.08 | 0.09 |
| rest-after | fix | 1 | 0 | 1 | 0 | 1776 | 0.08 | 0.1 |

### Anchor `bottom`

| Phase | Build | Native resizes | Moves | Size requests | Page resizes | Native bottoms (px) | Main % | WebContent % |
|---|---|---:|---:|---:|---:|---|---:|---:|
| rest | installed-src | 1 | 0 | 0 | 0 | 1626 | 0.53 | 0.07 |
| rest | parent | 0 | 0 | 0 | 0 | - | 0.24 | 0.02 |
| rest | fix | 0 | 0 | 0 | 0 | - | 0.18 | 0.04 |
| content | installed-src | 9 | 0 | 9 | 5 | 1218, 1626 | 1.68 | 0.76 |
| content | parent | 4 | 0 | 4 | 4 | 1068, 1776 | 1.65 | 0.72 |
| content | fix | 4 | 3 | 4 | 0 | 1912 | 1.62 | 0.65 |
| wrapping | installed-src | 10 | 0 | 10 | 6 | 1218, 1626 | 2.02 | 1.07 |
| wrapping | parent | 6 | 0 | 6 | 6 | 1068, 1776 | 2.19 | 0.92 |
| wrapping | fix | 3 | 1 | 4 | 0 | 1912 | 1.38 | 0.76 |
| straddle | installed-src | 31 | 0 | 35 | 27 | 1218, 1626 | 5.07 | 2.43 |
| straddle | parent | 8 | 0 | 8 | 8 | 1068, 1776 | 3.82 | 1.71 |
| straddle | fix | 0 | 0 | 8 | 0 | - | 2.5 | 1.06 |
| titlebar | installed-src | 10 | 0 | 10 | 8 | 1218, 1626 | 2.45 | 1.72 |
| titlebar | parent | 2 | 0 | 2 | 2 | 1068, 1776 | 1.35 | 0.68 |
| titlebar | fix | 0 | 0 | 2 | 1 | - | 0.99 | 0.4 |
| rest-after | installed-src | 1 | 0 | 1 | 0 | 1218 | 0.38 | 0.14 |
| rest-after | parent | 1 | 0 | 1 | 1 | 1776 | 0.11 | 0.09 |
| rest-after | fix | 1 | 1 | 1 | 1 | 1912 | 0.1 | 0.08 |

### Anchor `fixed`

| Phase | Build | Native resizes | Moves | Size requests | Page resizes | Native bottoms (px) | Main % | WebContent % |
|---|---|---:|---:|---:|---:|---|---:|---:|
| rest | installed-src | 0 | 0 | 0 | 0 | - | 0.15 | 0.01 |
| rest | parent | 0 | 0 | 0 | 0 | - | 0.22 | 0.02 |
| rest | fix | 0 | 0 | 0 | 0 | - | 0.36 | 0.02 |
| content | installed-src | 0 | 0 | 4 | 0 | - | 1.36 | 0.75 |
| content | parent | 0 | 0 | 4 | 0 | - | 1.47 | 0.71 |
| content | fix | 0 | 0 | 4 | 0 | - | 2.0 | 0.86 |
| wrapping | installed-src | 0 | 0 | 4 | 0 | - | 1.41 | 0.78 |
| wrapping | parent | 0 | 0 | 4 | 0 | - | 1.74 | 0.99 |
| wrapping | fix | 0 | 0 | 4 | 0 | - | 1.92 | 0.91 |
| straddle | installed-src | 0 | 0 | 8 | 0 | - | 2.12 | 0.93 |
| straddle | parent | 0 | 0 | 8 | 0 | - | 2.87 | 1.04 |
| straddle | fix | 0 | 0 | 8 | 0 | - | 2.73 | 1.2 |
| titlebar | installed-src | 0 | 0 | 2 | 0 | - | 1.65 | 0.49 |
| titlebar | parent | 0 | 0 | 2 | 0 | - | 1.07 | 0.59 |
| titlebar | fix | 0 | 0 | 2 | 0 | - | 1.28 | 0.52 |
| rest-after | installed-src | 0 | 0 | 1 | 0 | - | 0.11 | 0.05 |
| rest-after | parent | 0 | 0 | 1 | 0 | - | 0.16 | 0.09 |
| rest-after | fix | 0 | 0 | 1 | 0 | - | 0.07 | 0.07 |

### Reading the tables

- **`fixed`**: no native resize or move in any phase on any build. The bottom edge cannot move, which matches the owner's saved window.
- **`bottom`**: after the correction every native event reports bottom 1912, the 956-point work-area edge, while content, wrapping and titlebar change. `parent` flips between 1068 and 1776.
- **`top`**: this anchor grows downward by design, so the bottom follows the content and clamps at 1912. The same content gives the same height, and `fix` has no height flip; `parent` flips between 1164 and 1680.
- **Straddle**: `fix` has 0 native resizes for 8 size requests in `top` and `bottom`, because every count is clamped to the same maximum. `parent` has 8 resizes for the same requests.
- **Titlebar phase bounds rows**: `fix` and `fixed` samples show 5 or 6 short window-bounds changes with no native resize. In the `fix` `fixed` samples they fall within about 0.35 seconds after `VISIBLE 0` and `VISIBLE 1` and return to the same frame, which fits the system show and hide animation.
- **CPU**: at `parent` the loop is already absent, so `parent` and `fix` CPU are close. The saving of the cause itself shows against `installed-src`.

The loop at `installed-src` is not constant. In this run it stopped by itself before the final rest. An earlier run with an older instrument still had 82 resizes there.

## Live installed window

`runs/installed-live.jsonl` is 90 seconds at 30 Hz, read-only, of `4.6.77-261104-7`, with `window-sampler.swift`:

- 0 bounds changes; frame x 0, y 312, 354 x 634 points, bottom 946 points.
- Main 26.19%, WebContent 2.74% and 12.60%.

Two earlier samples of `4.6.77-261104-6` with an older sampler format are `installed-live-6-90s.jsonl` and `installed-live-6-900s.jsonl`: 90 s with main 18.75%, and 900 s with main 31.76%, both with 0 bounds changes.

The owner's window is pinned with a Fixed size, so its frame does not move and this rest CPU is **not** frame resizing. Its cause is not established here. Snapshot mode has no ticks, and `fixed` rests at 0.15% main, but that does not attribute the live load.

## Native input

`native_input.py` drives a separate process, `native-input.swift`, which posts real mouse events. See `runs/<build>-native-<anchor>/summary.json`.

| Check | `parent` | `fix` |
|---|---|---|
| Close button click | `close_requested`, fixed and top | `close_requested`, fixed and top |
| Manual grip, `fixed` | 350x616 to 400x676 points | 350x616 to 400x676 points |
| Fixed choice saves | 320x400, window 400x644 client: **wrong** | 400x644, window 400x644 client |
| Titlebar pixel alpha | 0 | 255 |
| Titlebar hit test | `NSTitlebarView`, can move window | `NSVisualEffectView`, can move window |
| Titlebar drag | Window unmoved, drag reached the window below; see below | Window moved: fixed 40,40 to 53,79; top 40,40 to 90,79; 1 focus and 13 to 15 move events |
| Copy widget | `storefront#100` on the pasteboard | `storefront#100` on the pasteboard |

Copy clicks reach a real `WKWebView` and the real general pasteboard. The helper keeps earlier items in memory, restores them right after reading, and never logs them. Every `restored` value is `true`.

### Titlebar drag and a side effect of this qualification

Dragging a titlebar is run by the window server, so events posted to a single process cannot start it. The helper therefore has a system-level `hid-drag`.

The first version of that guard checked only session idle and that the window was frontmost. On `parent` it performed two drags that did not move the test window and did not focus it. The window server passes clicks through fully transparent pixels, so the drags reached the windows below, and another application's window was left displaced by exactly the drag distance of 60 and 40 points. The guard now also reads the start pixel and refuses a transparent one, so the committed `parent` runs show `performed: false`.
The later guarded reruns also overwrote the raw logs of the performed drags. `runs/drag-performed-session-output.txt` keeps the four summary lines as the session printed them, so these drags have summaries only. The committed `fix` native runs show `performed: false`: later attempts were refused because the session was active or, with an idle session, because the target window was not frontmost at the start point. That refusal's cause was not established. The commanded drag was +60,+40; the fixed run moved +13,+39 and the top run +50,+39, so these drags prove the window moves and do not prove exact tracking.

This is also a product finding: **at `parent`, with the transparent titlebar, a real drag on the empty titlebar area does not move the TaskQ window** and acts on what lies beneath it. The correction removes this, because its titlebar pixels are opaque in Background mode. In Whole mode at 80% they keep alpha 204. Whole-mode dragging was not run.

## Visuals

`captures/` holds 10 native window captures from `screencapture -x -o -l`, 5 per build, with invented fixture data only. The capture composites no desktop, so transparency is in the PNG alpha.

| Mode | `parent` titlebar | `fix` titlebar | Body, both builds |
|---|---|---|---|
| Dark, background 100 | 0,0,0,0 | 20,20,20,255 | 38,43,49,255 |
| Dark, background 35 | 0,0,0,0 | 20,20,20,255 | 38,43,49,147 |
| Light, background 35 | 0,0,0,0 | 236,236,236,255 | 252,252,252,147 |
| Dark, whole 80 | 0,0,0,0 | 20,20,20,204 | 38,43,49,204 |
| Light, whole 80 | 0,0,0,0 | 236,236,236,204 | 250,251,253,204 |

Browser and static proof stay with the parent task and are not part of this evidence.

## Checks on the patched source

`cargo test --locked --bin taskq tray` on `parent` plus `correction.patch`: 66 passed, 2 failed, 2 ignored. The same 2 fail on the pure parent tree on this host: `blocked_capacity_does_not_block_pause` and `workspace_capacity_apply_drops_reader_stamp_and_keeps_strict_payload`. They are unrelated to this patch and the parent task should confirm them.

## Hashes

| File | sha256 |
|---|---|
| `correction.patch` | `b1272bc089b45b769dff62401853f73ef46bdfc11213ac132b028177121b9eb1` |
| `instrument.patch` | `e40d00ed9d0b3b2ea5f8ea4a8b6d67523444a4694d681853cb07145a92f6551e` |

## Limits

- CPU values are from dev-profile builds in snapshot mode with no ticks and no tray icon, so they compare builds and do not predict release load.
- The temporary binaries were not installed and the correction has not run as the owner's real window.
- Position saving after a drag is not exercised, because that handler is off in snapshot mode.
- `native_titlebar` finds the titlebar through the Close button's superview. That is view-hierarchy knowledge, so a failure logs one stderr line and leaves the window unchanged.
- The Mac is the only host tested; Windows and Linux behaviour rests on `client_size` compiling to `inner_size` there.
- Close clicks are window-targeted posted events, not system-level clicks.
- The pasteboard restore is in memory and covers the items read before the click.
- CPU figures come from `proc_pid_rusage`, because `ps` and `top` are denied here. Pictures and coalesced telemetry are not used for CPU causality.
- The `installed-src` baseline is built from source plus the instrument, not the installed binary. The installed binary has only the read-only live sample.

## Reproduce

```sh
git apply docs/design/evidence/2375/instrument.patch   # in a temporary copy of the parent tree
git apply docs/design/evidence/2375/correction.patch   # for the fix build
swiftc -O docs/design/evidence/2375/window-sampler.swift -o window-sampler
swiftc -O docs/design/evidence/2375/native-input.swift -o native-input
python3 docs/design/evidence/2375/qualify.py BIN window-sampler docs/shots/fixture.sh OUT LABEL top
python3 docs/design/evidence/2375/native_input.py BIN native-input docs/shots/fixture.sh OUT LABEL fixed
```

`native_input.py` uses the system-level drag when the guard allows it, so run it only on a host where a moved cursor is acceptable.
