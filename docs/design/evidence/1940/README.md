# #1940 Reports checkpoint

The owner widened #1940 to one Results flow for completed tasks and reports. Results implementation remains incomplete; the task's ask resolves Reopen before changing the closed-task model (R13/§2). These artifacts preserve the completed Reports investigation and implementation.

## Baseline

- Installed Windows v4.6.47: `taskq reports --json` took 70,540 ms for 9 reports. [Per-call observations](before-calls.json): 3,680.083 ms discovery, 95 comment-list calls totalling 66,493.939 ms. The supplied Mac observation was 65 s for 9 reports; not rerun here.
- [Before native identity](before-native.json): installed executable SHA-256 recorded. The exact installed source revision was not independently verified.
- [Before menu](before-windows-menu/popover-dark-reports-16.png) and [before Conversations](before-windows-conversations/popover-light-reports-conversations-16.png). Each route also retains its other theme. Invented report data; actual Windows WebView2 renderer, 16 px.

## Reports implementation

- [After menu](after-windows-menu/popover-dark-reports-16.png) and [after Conversations](after-windows-conversations/popover-light-reports-conversations-16.png). The card exposes Question, Recommendation and Proposed tasks before findings; Full report and Make tasks/Later/Drop are direct controls. Both themes are retained: an eight-screen before/after review set across two routes.
- [After native identity](after-native.json): source `baf107d0`, binary SHA-256, route, exit status and capture time. Conversations fixtures scroll to the action area on both versions. Later changes add explicit timing capture; these pictures do not qualify that telemetry change.
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

`cargo test --locked --bin taskq reports::tests`: 11 pass. `cargo test --locked --test integration reports_cli`: passes, including committed timings. Reports component/receipt tests pass. The 16-cell browser instrument covers Menu/Conversations, dark/light, 350/940 px, 16/20 px. Its redundant intermediate visibility wait was replaced by a mounted-row barrier; the subsequent real click still requires visible, stable controls, and geometry checks remain.

Full gate attempts and the final in-flight gate are recorded in the preservation handoff. The first passed Rust/integration/selftest and failed stale generated web hashes after a separator correction; the next passed those and Node checks, then timed out at the intermediate browser visibility wait. A focused rerun passed. No failed gate is reported as a pass (R12).

`measure.py` recreates nine invented collected reports in an isolated installation and requires committed list/open timings. The earlier [cached read observation](after-cached.json) predates explicit action timing capture; it is not final performance qualification. Production collection and the expanded completed-task flow remain unverified.
