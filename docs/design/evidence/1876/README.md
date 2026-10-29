# Menu transparency · #1876

M1.1 now permits 60–100% whole-window opacity in 1% steps. M1.2 fades all menu, block, card, chrome, footer and popover fills without a backing floor. Text, icons, buttons, badges, input boundaries and control glyphs stay opaque in Background only. Input fills and decorative borders/shadows follow background opacity.

## Applied decisions and protection

R13: the root contract points to M1.1/M1.2; their specifications and visual treatment were updated before implementation. These are the two amended decision IDs. R1–R13 remain unchanged. The owner explicitly replaces the old 90% limit and 98% backing floor in A1–A3; no other visual reference, layout, icon or color decision changes.

R12 and § 7: model, browser, native and publication evidence remain separate. No running installation was replaced. Windows WebView2 is Not tested: no accessible Windows desktop was provided to this attempt. Owner visual acceptance and merge-result `merge_group` CI remain pending the reviewer/publication flow.

- Before: `node tests/tray-settings.test.cjs` rejected the expected `min="60"`; the rendered range still said `min="90"`. After: Pass with the same command. Existing sign-in, access, routes, recovery and Off checks remain.
- The fill check caught an opaque hovered task-title link. Its shared hover token now follows background alpha. Explicit hover at 0% and 50% prevents recurrence.
- The retired backing setting is ignored on load and omitted from renderer state. Saving preserves its inert legacy field and unrelated settings. Rust checks cover 60, 61, 89, 90, 95 and 100 across save/reload; malformed input is still refused before application. Mode switching restores each independent setting and uses native alpha 100% for Background only.
- The obsolete 98% backing/AA assertions are replaced by computed fill-alpha and full-foreground-alpha checks on every affected route, including popovers, shared bodies and disabled controls. Mutation checks restore a 98% content backing or fade a control/SVG/ancestor and require failure. Contrast protection remains at background 100% and whole-window 90–99%; lower whole-window values and an unbacked desktop do not promise AA on arbitrary wallpaper.

## Native capture

UI source: `ab884a42aa53b101c3807d237ad2d11bce5518a4`. The root preamble was restored to its original text after review; every captured UI source file is unchanged. Based on `fa212bc16e32da742f027c11ca42ac26c59ea3a7`. macOS Darwin 27.0.0 arm64, 2× display, WKWebView, Node 22.18.0 and Cargo 1.98.1. The signed cargo runner starts the candidate. `docs/shots/fixture.sh` supplies invented projects in an isolated installation; no board actions or real agent sessions run.

Commands, with example temporary directories:

```sh
sh docs/shots/fixture.sh /tmp/taskq-opacity-fixture
TASKQ_INSTALL=/tmp/taskq-opacity-fixture/install cargo run --locked --bin taskq -- tray --snapshot /tmp/taskq-opacity-shots --route opacity
swift docs/design/evidence/1876/alpha.swift docs/design/evidence/1876/native/screen*.png
```

The opacity route exercises Whole window 100%, 90%, 60% and Background only 0%, 50% in both themes. The review set retains the six requested 60%/0%/50% window captures. Each uses native `screencapture -x -o -l <owned NSWindow id>`, with a temporary owned busy backdrop window behind the candidate: repeating 24-point black, white, system-blue and system-orange squares. The system wallpaper is unchanged. A window capture retains alpha rather than baking the backdrop into its PNG.

The first occluded capture contained only alpha 0 despite a nonempty WKWebView image: [failure](native/before-occluded.png). It is not accepted evidence of transparency. The opacity snapshot window now stays above other windows; ordinary snapshot placement and installed window behavior are unchanged. The repeat contains foreground pixels and the expected background/native alpha. [Capture identity and hashes](native/capture.json); [all 256 alpha-bin counts](native/alpha.json). The histogram instrument draws each PNG into premultiplied RGBA8 and counts every alpha byte; its assertion requires exactly width × height samples.

| Case | Light and dark native observation | Result |
|---|---|---|
| Whole window 60% | 1,126,220 pixels at alpha 153/255 per image; no alpha-255 pixels | Pass |
| Background only 0% | Dark: 748,311 clear / 320,588 opaque pixels; light: 752,184 clear / 316,978 opaque pixels | Pass |
| Background only 50% | Dark: 643,687 pixels at alpha 128; light: 647,483 at 128; opaque foreground remains | Pass |

- Whole 60%: [dark](native/screen-popover-dark-whole-60-16.png), [light](native/screen-popover-light-whole-60-16.png).
- Background 0%: [dark](native/screen-popover-dark-background-0-16.png), [light](native/screen-popover-light-background-0-16.png).
- Background 50%: [dark](native/screen-popover-dark-background-50-16.png), [light](native/screen-popover-light-background-50-16.png).

All six native window images were inspected individually. Appearance, mode selection, enabled/disabled ranges, inactive-mode hints, chrome, footer, text and control glyphs remain visible without clipped essential controls. Zero-opacity images intentionally have no reading backing. The browser check separately covers task blocks, cards, hover and open popovers at 0%/50%, both themes and 320px / 20px text. Separate Settings, View and Conversations windows remain outside the menu effect. Native click/raise behavior is not claimed. Supplemental region-capture synchronization could not inspect the candidate process because `ps` was sandbox-blocked; no permission override was attempted. The accepted native evidence is the six `-l` captures with histograms, not the unsynchronized supplemental region images.

## Checks

- `node tests/tray-settings.test.cjs`: Fail before; Pass after.
- `cargo test --locked --bin taskq tray::tests::opacity`: Pass (persistence/range and retained 90% AA palette).
- `cargo test --locked --bin taskq tray::tests::background_opacity`: Pass (legacy load, independent modes, invalid IPC).
- `cargo test --locked --bin taskq tray::tests::snapshot_routes_capture_only_named_pages`: Pass (bounded ten-case opacity instrument).
- `node tests/tray-opacity.browser.cjs`: Pass (60%, 1% keyboard steps, Off, IPC, shared text and window isolation).
- `node tests/tray-background.browser.cjs`: Pass after the hover fix (fill/foreground checks, mutation sensitivity, mode hints and window isolation).
- `node tests/task-workspace.test.cjs`: Pass (same released bundle and stylesheet).
- Canonical selected gate: first run failed only `core::tests::every_table_row_has_a_golden_trace`: the added root link made `taskq.md` 61,504 bytes, above the R8 60 KiB limit. Restoring the original root contract keeps it at 61,434 bytes; the limit and its assertion are unchanged. The repeat is justified by that failure, not by a move of main. Its outcome follows below.


Canonical repeat: `python3 scripts/check.py --base fa212bc16e32da742f027c11ca42ac26c59ea3a7` completed in 49.487 s on this Mac with a warm cache. **sandbox-blocked, not Pass**: 674 unit tests passed, 35 ignored; integration had 19 passed, 1 ignored and 1 sandbox-blocked (`telemetry_process::resume_records_its_result_without_releasing_the_host_hold`: `sandbox-exec: sandbox_apply: Operation not permitted`). No sandbox or permission override was attempted. The rule gate accepts only the named M1.1/M1.2 changes. The first run's R8 size failure is fixed; its focused rerun passes.

The remaining selected checks ran separately because the sandbox refusal stops the canonical runner: all `tests/*.test.cjs`, screenshot-manifest checks, release-tag/exact-source checks and both changed transparency browser instruments passed. Offline selftest in an isolated installation passed 107 golden traces, owner-text formats and committed telemetry capture. Its board and tool probes are intentionally unknown/offline. No tests or assertions were relaxed for the sandbox refusal. Review set: six successful native images plus one distinct failed capture, seven screens total.

Review correction (R13, § 7): byte comparison against the original base failed before restoring `[Alpha][menu]` and passed after. The root contract now has no task diff. No implementation, test, dependency or native artifact changed in this correction; the earlier checks and captures remain applicable. The focused R8 trace/size check and rule gate pass without changing a guarding assertion.
