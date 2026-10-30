# #1940 Results evidence

Results includes completed tasks and reports. [The owner decision](https://github.com/taskqorg/taskq/issues/1940#issuecomment-6526379730) defines Reopen as a linked held successor carrying the reason; the original stays closed. The Results specification records that flow under R13. §2 and §7 publication remain unchanged.

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
- macOS native: [#1957 evidence](https://github.com/taskqorg/taskq/blob/0e2d08e6fcee94e692fe0778719ca6bfbc1a4e71/docs/design/evidence/1957/README.md) supplies the required A7 before/after images and walkthrough for both result kinds and surfaces at this PR's unchanged implementation. See the final acceptance map below. PilotQ-side delivery remains Not tested; only the JSON CLI boundary is verified.

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

## Review repair

Rebased the existing branch onto `ae6fc6c71130e7224f83e24083033b3a5be0f4b8` after the review returned its merge conflicts. The Reports specification and action adapter retain main's automatic claim-issue setup before Make reservation, and the fake board retains the publication adapter alongside the paging-clock fixture. R13 is restored verbatim; the proposed Results link now belongs under Product. The normalized contract is 297 lines and 61,433 UTF-8 bytes, within R8's 300-line/60-KiB bound.

`python scripts/check.py --rules --base ae6fc6c71130e7224f83e24083033b3a5be0f4b8` refuses: `the task does not name Product (changed)`. At that checkpoint, the issue named R13 but no owner approval named the Product rule ID. The Product addition was a proposal awaiting approval; no rule-gate exemption was added. The earlier full-gate result above does not qualify the rebased candidate. A new full gate must follow the recorded Product decision.

Focused verification after conflict resolution: `cargo test --locked --bin taskq reports::tests` passed all 15 tests; `cargo test --locked --bin taskq gitlab_report_make` passed the preserved automatic claim-setup/held-creation path; `cargo test --locked --bin taskq core::tests::every_table_row_has_a_golden_trace` passed trace coverage and contract size. `git diff --check` passed. These are focused source checks; the rebased full gate, macOS evidence #1957 and merge-result CI remain outstanding. No test assertion or gate was weakened during this repair.

[Owner answer 6527970809](https://github.com/taskqorg/taskq/issues/1940#issuecomment-6527970809) approves the Product link. This resolves the approval prerequisite above while retaining R13 verbatim. The next canonical gate checks that the recorded decision is recognized; no rule-gate code or assertion changes. macOS A7 evidence still requires #1957.

## Approved candidate gate

Candidate `c7df004d9f1ddd1b526554a038d016b73723fb73` passed `python scripts/check.py --base ae6fc6c71130e7224f83e24083033b3a5be0f4b8` on the local Windows host in **266.609 s / 900 s**, with no retries. The Product rule gate now accepts the recorded owner answer; the previous missing-Product failure is resolved without an exemption. Rust: 726 passed, 0 failed, 39 ignored. Integration: 19 passed, 0 failed, 1 ignored. Component, offline selftest and release checks passed. Both Reports and Results browser instruments passed all 16 cells, including selected-question completion routing. All five original failures, the extra-board-poll regression and the status-observation fixture passed.

This evidence-only update changes no tested source, tests, dependency or contract. Raw gate output remains in repository scratch `.taskq/gate-approved-product.log`. Offline board/tool probes and ignored tests remain unknown; this local run does not qualify cold CI or production behavior. At this checkpoint macOS evidence was blocked on #1957; the final acceptance map below records its delivery. Publication still requires green merge-result CI (R12).

## Final acceptance map

[#1957](https://github.com/taskqorg/taskq/issues/1957) closed after PR #1959 merged as `0e2d08e6fcee94e692fe0778719ca6bfbc1a4e71` with green merge-result CI. Its [native evidence package](https://github.com/taskqorg/taskq/blob/0e2d08e6fcee94e692fe0778719ca6bfbc1a4e71/docs/design/evidence/1957/README.md) tests #1956 head `1e9912569f9b7374ae119bd8da0534064619b3d2` on macOS 27.0.1 arm64, WKWebView 22625.1.29.11.28. That is the implementation handed off here; this continuation changes evidence prose only. The package pins baseline and candidate binary hashes, the invented fixtures and the disclosed baseline/walk harness patches. No production service or real decision was changed.

| Acceptance | Evidence and outcome |
|---|---|
| A1 map and baseline | Pass: report definition, Menu/Conversations/CLI/PilotQ boundary and close-to-decision steps in `docs/research/1940.md`; Windows per-call baseline above, supplied Mac baseline identified separately. |
| A2 spec first | Pass: `docs/design/reports.md` records the unified Results flow and purpose of each step; owner-approved Product link in `taskq.md`. R13 remains verbatim; no §2 lifecycle amendment. |
| A3 notice and seen state | Pass at component/browser/native fixture boundaries: pushed attention and Conversations rows before opening Results, unread counts and SHA-bound seen state; already seen items add no attention. |
| A4 card and reactions | Pass at component/browser/native fixture boundaries: decision-first reports and full report action; ordinary Changed/Checks/Open/PR; Make/Later/Drop and Accept/Follow-up/Reopen. Reopen creates a linked held successor with the reason; the original stays closed. |
| A5 background collection and local navigation | Pass at model/CLI/UI fixture boundaries: close invalidation, incremental collection, local list/open/Next/Previous and committed telemetry. macOS CLI list/open wall times 25–43 ms; cached operations 6.232–26.022 ms, zero board operations and byte-identical source status. |
| A6 unknown and identity | Pass at regression/native fixture boundaries: missing, stale, invalid and failed collection name explicit reasons; rows and failed-action receipts survive. SHA markers and Make/Later/Drop protection remain. |
| A7 platform evidence | Pass for the specified walkthrough: Windows review set above; macOS package has ten native screens and Menu 16/16, Conversations 15/15 scripted checks. No TCC denial. Ordinary Windows before images are Chromium, as disclosed; native macOS before/after covers both result kinds and surfaces. |

The macOS timings are isolated release-binary observations with nine invented reports and ten completed tasks, not a repeat of the live-board baseline or a production latency claim. Its full gate was sandbox-blocked in a telemetry integration test; it is not recorded as PASS. The unchanged implementation's Windows canonical gate above passed, and #1956 PR CI was green at the previous head. This evidence-only continuation does not rerun the full gate or rebase solely because main moved (§7; `docs/testing.md`).

Remaining limits (R12): real board reactions, installed production collection, PilotQ-side delivery, ignored checks and the native status-item click path are Not tested. PR #1959's green merge result qualifies the evidence deliverable only; #1956 still requires its own green merge-result CI before publication. No deployment or owner visual acceptance is claimed.

Continuation verification: `python scripts/check.py --base 1e9912569f9b7374ae119bd8da0534064619b3d2` selected documentation checks and passed in 0.953 s / 900 s without retries (invoked with the equivalent `--base HEAD` before committing). `git diff --check` passed. The only changed files are this index and `docs/research/1940.md`; implementation, tests, dependencies and contracts match the macOS candidate and the Windows full-gate source. Four pinned macOS before/after images were also visually inspected during this Windows handoff.
