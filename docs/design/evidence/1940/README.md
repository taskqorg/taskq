# #1940 Results evidence

Results includes completed tasks and reports. [The owner decision](https://github.com/taskqorg/taskq/issues/1940#issuecomment-6526379730) defines Reopen as a linked held successor carrying the reason; the original stays closed. R13 and the Results specification record that flow. §2 and §7 publication remain unchanged.

## Baseline

- Installed Windows v4.6.47: `taskq reports --json` took 70,540 ms for 9 reports. [Per-call observations](before-calls.json): 3,680.083 ms discovery, 95 comment-list calls totalling 66,493.939 ms. The supplied Mac observation was 65 s for 9 reports; not rerun here.
- Installed Windows `taskq status --closed --json` took [22,557 ms for ten completed tasks](before-completed-load.json). Per-call attribution for this second command was not recovered; its wall time is observed.
- [Before native identity](before-native.json): installed executable SHA-256 recorded. The exact installed source revision was not independently verified.
- Invented report data; Windows WebView2 renderer, 16 px. The review set below links one image per distinct comparison.

## Review set

- Reports before: [menu](before-windows-menu/popover-dark-reports-16.png), [Conversations](before-windows-conversations/popover-dark-reports-conversations-16.png). Windows WebView2, installed executable.
- Reports after: [menu](final-windows-reports/popover-dark-reports-16.png), [Conversations](final-windows-reports-conversations/popover-dark-reports-conversations-16.png). Question, Recommendation and Proposed tasks precede the full report; Make/Later/Drop remain direct actions.
- Completed tasks before: [menu](results/before-windows-tray-dark.png), [Conversations](results/before-windows-workspace-dark.png). Windows Chromium rendering HTML extracted from the installed executable: Completed offered a board/PR link, and Conversations had no completion row. This is not a native before capture.
- Completed tasks after: [menu](final-windows-results/popover-dark-results-16.png), [Conversations](final-windows-results-conversations/popover-dark-results-conversations-16.png). Windows WebView2. Changed, Checks, Open, PR and Accept/Follow-up/Reopen share the result card.

These eight screens cover distinct before/after decisions across two result kinds and two surfaces. Light-theme captures are diagnostic artifacts outside this review set; the automated matrices cover theme, width and text-size variants. [Final native identity](final-native.json) names the exact source, binary SHA-256, routes and outcomes. Previous [Reports checkpoint captures](after-native.json) remain historical evidence.
- Native effects use invented receipts. No real report was acknowledged or decided, no task was created and no host was installed or restarted. Cargo supplies the GNU debug loader paths; a bare debug invocation returned `0xC0000135`.
- macOS native and PilotQ-side delivery: **Not tested**. This Windows attempt has no macOS surface; the available PilotQ boundary is the JSON CLI, not a verified PilotQ walkthrough.

## Failure and protection

| Requirement/risk | Failure before | Proof after |
|---|---|---|
| A5 local navigation | `opening Reports must not request a board list`: 1 request instead of 0 | `reports-cache.test.cjs`; real CLI read without a board credential in `reports_cli.rs` |
| A6 close order | Read receipt replaced close time 40 with 0 | Marker-only overlays preserve source and close order |
| A6 source identity | Late SHA A acknowledgement was merged with SHA B body | SHA-scoped receipts and selected-row matching |
| A6 partial creation | Stale collection erased a Make reservation | Error receipts survive redraw; newer collected comment revisions supersede old overlays |
| A5 freshness | Same-second updates could be reused indefinitely | Board-time settled-read regression plus close-event scheduling |
| A5 telemetry | [Initial isolated export](telemetry-before.json) had no named cache timings | Explicit action capture; real CLI and component checks now require committed named timings |
| A5 paging/restart | A post-list cursor could skip a concurrent update; restarting without retained entries could drop a failed row | Pre-list watermark and deterministic paging-window trace; hydrated-row failure check |
| Completed evidence | A wrong-attempt close could replace the displayed verdict | Accepted transition selects the close; wrong-attempt verdict/merge regression |
| Reopen recovery | Renaming the source changed its immutable create payload; late receipts could erase another result's draft | Stable successor title and rename replay; source/SHA-scoped receipt and draft checks |
| Background board budget | Unified gate: deferred handover listed 6 times where the unchanged assertion permits at most 4 | Collection completion now uses the existing local sleep tick to publish status, without poking another board pass |

`cargo test --locked --bin taskq reports::tests`: 15 pass. `cargo test --locked --test integration reports_cli`: passes, including the `results` alias and committed timings without board credentials. Component checks retain SHA-bound receipts and protect late successor responses from clearing another result's draft. Each browser instrument covers Menu/Conversations, dark/light, 350/940 px and 16/20 px: 16 cells. Results also covers a selected question becoming a completion, populated/empty unknown Completed history, and legacy answered reports retained only in history. The report instrument's mounted-row barrier retains a real visible/stable click and geometry checks.

Full gate attempts did not pass (R12). The first passed Rust/integration/selftest and failed stale generated web hashes after a separator correction; the next passed those and Node checks, then timed out at the intermediate browser visibility wait. A focused browser rerun passed. The final `python scripts/check.py --base 04e43344db6dafcd94c46e06ecd13d908ebcefa2` ended after 447.797 s: 701 Rust tests passed, 5 failed, 39 were ignored; later gate phases did not run. Failures:

- `link::tests::a_read_crosses_the_fake_relay_sealed_and_fenced`
- `runtime::app_control::tests::cancellation_is_owner_bound_bounded_and_never_replays_work`
- `runtime::tests::file_stdin_handoff_preserves_bytes_and_never_waits_for_a_reader`
- `telemetry::tests::older_tray_writer_hands_over_to_new_host`
- `tray::tests::tray_collector_recovers_after_failed_election`

All five failures concerned elapsed waits or child readiness. The failed run also recorded unrelated tests beyond 60 seconds. Host contention is a hypothesis, not a proved cause. The checker now defaults to four Rust harness threads on Windows, with explicit overrides preserved; all cases, internal concurrency, ownership/byte/receipt assertions, deadlines and the 900-second gate budget remain. It also restores the documented Playwright dependency after `npm ci` when local browser instruments are selected. The final full-gate outcome is recorded below; earlier successful runs do not substitute for it.

`measure.py` recreates nine invented reports and ten completed tasks in an isolated installation and requires committed list/open timings. [Final cached reads](after-cached.json) distinguish operation time from wall time including Cargo/process startup. The earlier missing-timing export is preserved in `telemetry-before.json`. Production collection, macOS native behavior and PilotQ delivery remain unverified; no deployment or live board decision is claimed.

## Final gate

The first unified Results gate took 199.390 s: 708 passed, one failed, 39 ignored. All five earlier deadline failures passed. Its remaining failure was `host::tests::deferred_handover_lists_the_board_once_per_poll`: collector completion pokes forced extra board lists. The local publication-tick fix retains that test and its threshold; its focused reproduction passes afterward. The next gate passed Rust, process, component, release and Reports browser checks, then failed a strict selector matching both Close and Done in the new Results browser instrument. Selecting the actual Done button retains the history assertion; the focused 16-cell run passes. Native pictures and cache-operation timings name source `49940ae2`; subsequent fixes change publication scheduling and legacy archive retention, with the rendered cards unchanged.

The archive candidate's next gate passed 710 unit checks, then its status-observation integration fixture timed out in an unattributed 10s Windows process wrapper. This wrapper now matches the existing host CLI fixture budget: 60s on Windows, 10s elsewhere, without retries or changes to H-A1/H-A5 ownership/generation/unknown assertions or native deadlines. A focused run passed all 14 invocations (0.158–1.170s each, 14.95s total). The previous stall's cause is unknown; this is fixture-budget alignment and attribution, not a claimed product latency fix.

The frozen code candidate `a8c53a18ba33ba359b866ab2030f5dbdde2aa7da` passed `python scripts/check.py --base 04e43344db6dafcd94c46e06ecd13d908ebcefa2` on Windows in 206.703 s of the 900 s budget, with no retries. Rust: 710 passed, 39 ignored; integration: 19 passed, one ignored. Component, offline selftest, release checks and both 16-cell browser matrices passed. The five original failures, board-poll regression and status-observation fixture all passed in this run. Ignored checks and offline board/tool probes remain unverified (R12). The final evidence-only commit records this outcome without changing the tested code.
