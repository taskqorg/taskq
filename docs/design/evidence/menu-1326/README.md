# Menu pass evidence

Task #1326, attempt 6283338815. Implementation candidate; visual acceptance and publication remain pending.

## Specification and behavior

Applied R13 and § 7: M1/M3/M13/M14/M-A21, I1/I4 and the shared Custom schema describe the changes. The owner's answer selects the existing compact Q only when Summary would otherwise be empty. Old Custom `ready` bindings and configurations without the optional fields retain their meaning.

The header is outside the scrolling content. Home and Settings move into it; Pin, Top, size anchor and Close have accessible names. Size modes persist and constrain their named edge to the work area. Native window policy follows Pin. The macOS application bundle includes the TaskQ icon in four ICNS representations.

## Before and after

Baseline: `3c6e2a65`. `before/` uses the repository screenshot fixture and native WKWebView, `tray --snapshot DIR --route home,icon`. `after/` contains the first implementation draft; `candidate/` contains the final implementation capture. These are fixture observations, not installed-service qualification. Candidate UI sources are commit `4dad5e60551da7e63c250ea0eff6d81893fce236`; README provenance is in `docs/shots/manifest.json`.

Before: [dark](before/popover-dark-home-16.png), [light](before/popover-light-home-16.png). Candidate: [dark 20 px](candidate/popover-dark-home-20.png), [light 16 px](candidate/popover-light-home-16.png). These candidate images, the refreshed README project image and both active Summary icons were visually inspected: controls and content remain visible without overlap. This is not owner acceptance.

`empty/` uses the same fixture with tasks and events cleared to show the approved compact-Q fallback.

- Before: the actual controller's `head()` failed `assert.match(..., /data-route="home"/)` (A2). After: the navigation component check passes with Home and Settings in the header.
- Before: the content-fit browser check failed when a tall viewport navigated to shorter content. After: measurement uses the inner content block; the same check passes.
- Before the owner decision: zero counts and empty history produced a transparent active Summary. After: the same case equals the existing compact Q pixels. Unknown still draws a dash; nonempty Summary does not gain a Q overlay.
- Geometry tests retain nonzero ink alignment, graph scale, pause and unknown protections; zero-cell expectations follow the approved I1 change. The new checks retain Custom binding compatibility and deduplicate blocked tasks.

## Verification and limitations

- `python3 scripts/check.py --base origin/main`: sandbox-blocked at the full Rust suite under worker authority. 397 passed, 16 failed, 34 ignored. Failures include explicit worker refusals for host installation, host policy and Stop/restart; related receipt expectations fail. No authority flags were removed. This is not a passing canonical gate.
- Offline `cargo run --locked -- selftest`: 85 fold traces and format checks pass; telemetry commits. Board remains unknown because the offline flag is set.
- Focused checks pass: five `summary_` tests, the anchor geometry test and persisted Pin/position test. All Node component checks pass, plus the window, Home proposal and answer browser instruments. Browser checks are Chromium evidence, not native rubber-band proof.
- An isolated macOS candidate was exercised through native controls: content scrolled beneath the stationary header; Top toggled independently; Fixed, bottom and top modes cycled; Pin toggled. Its saved settings reflected these operations. The fixture was stopped afterward. This does not qualify the installed application, notification banners or Windows.
- Both window-targeted and rectangle-targeted native video capture exited 1 without diagnostics. No recording was produced; the cause is unknown. OS permissions were not changed. A8's continuous bounce/control recording remains blocked.
- Native Cmd+Tab/Alt+Tab and notification-banner icon acceptance remain unverified. Packaging/source changes do not prove those interactions.

Nothing is installed or deployed. Owner visual acceptance, the missing native evidence and green merge-result CI remain required before publication.
