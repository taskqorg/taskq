# Activity Ask marks · #1725

R13: the Product decision and I4/I5 were amended before implementation. The shared SVG keeps Ask marks hidden; only the Activity projection reveals them. Closed height/peak, failed caps, zero, unknown and ongoing marks retain their meanings. No numbered R-rule changed.

## Failure and retained protection

Before the fix, `cargo test --locked --bin taskq history_ask_marks_preserve_closed_zero_unknown_and_icon_pixels` failed: expected three Ask marks (ask-only, mixed, incomplete), observed zero. The browser regression also failed on the old fixture: expected five observed Ask bins, found zero marks.

After the fix, the same Rust regression passed, together with the existing icon tests (5 passed, 1 subprocess helper ignored). `node tests/tray-history.browser.cjs` passed both themes at DPR 1/2, including synthetic ask-only/mixed/zero/unknown bins, legend totals, accessible per-bin counts, keyboard selection and picker hiding. Existing Closed scale and raster/palette protection remain; the new test additionally compares icon pixels with and without Ask events at 256 px.

## Canonical gate and limits

`python3 scripts/check.py` initially found the Product sentence exceeded R8's 60 KiB bound (61,510 bytes). It was shortened without changing meaning: 61,437 bytes and 299 lines; `cargo test --locked --bin taskq every_table_row_has_a_golden_trace` then passed.

The final canonical invocation ended after 25.486 s: 541 Rust unit tests passed, 17 failed at inherited worker-role guards, 32 were ignored. Failures include `a worker cannot change host policy`, `a worker cannot install the host service` and `a worker cannot restart the host service or request Stop`. Record these host checks as **sandbox-blocked (worker-role context), not PASS**. The attempt environment and guards were not removed. Integration targets after that failure were not reached; CI remains required by R12/§7.

The remaining checks were run separately: isolated offline `cargo run --locked -- selftest` passed (99 fold traces, format checks and committed telemetry; board read intentionally offline); all 25 Node component scripts, screenshot/release script checks and the Activity browser instrument passed. The intended CSS/copy changes also required refreshing `task-workspace.sources.json`; its checksum assertion then passed without weakening protection.

## Installed Mac candidate

The signed release candidate was copied into an isolated temporary `TaskQ.app` installation using the product bundle layout and identity, verified with `codesign --verify --deep --strict`, then started through Cargo's runner. The production installation, service and board were not changed. The product `tray --snapshot --route cadence` read disposable project event files; no JS state injection or mock renderer was used. The captured UI source blobs remain identical in the final candidate. [Capture metadata](capture.json) pins the source commit/blobs, installed binary hash, OS, scale, exact event timestamps and image hashes.

Custom interval: 3600 s, six 600 s bins. Coverage starts 300 s after the interval begins. Invented events at offsets 500/700/800/900/1500 s are Ask; 1300/1400/2500 s are Closed; 1550 s is Failed. This yields observed totals Closed 3, Failed 1, Ask 5, Blocked 0; the incomplete first bin makes totals lower bounds. The interval ends 300 s after capture setup, retaining an ongoing final bin.

| Acceptance | Observed result |
|---|---|
| A1 | Product and I4/I5 specify History-only Ask; menu-bar/Dock pixels and picker hiding remain protected |
| A2 | Bin 2 shows Ask 3 with Closed 0; bin 3 shows Ask 1, Closed 2 and Failed 1; Closed peak remains 2 |
| A3 | Bin 1 shows unknown coverage plus observed Ask; bin 4 shows a zero tick; bin 6 has a distinct ongoing underline; accessible counts and legend agree |
| A4 native | Pass in installed isolated Mac candidate; dark 16 and light 20 retained below; no TCC prompt or horizontal-overflow failure |

[Dark 16](popover-dark-cadence-16.png) · [Light 20](popover-light-cadence-20.png). Both screenshots were opened and visually inspected: separate Ask/Failed marks and Closed bars are visible; no lost series or clipped plot. Two screens are retained, below the four-screen limit. The README screenshot manifest was refreshed with `docs/shots/update.sh` per §7.

R12 limits: this proves the isolated installed candidate's WKWebView rendering. It does not prove a production rollout, Windows native rendering, owner visual acceptance or merge-result CI. Final canonical-gate and publication outcomes are recorded in the task result (§7/R5); merge_group remains the publication gate.
