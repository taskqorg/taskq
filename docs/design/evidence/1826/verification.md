# Reports acceptance (#1826)

Finished research delivers a reviewed report. The owner can read it later and create held follow-ups without keeping finished research in the queue. A1–A5 are implemented. A6 now has four Windows and four macOS native frames, plus two before frames (10 total). The task result records the final candidate gate; publication still requires green merge-result CI (R12, §7).

## Contract and scope

- The owner answer on #1826 authorizes the amended research gates in §7 and §7 Result and close verdicts. R13's linked rule-ID detail accepts ordinary section citations while preserving section/subsection boundaries. No other numbered rule changes.
- R1 is preserved: trusted issue comments own read/decision state and immutable create claims; there is no local report decision store. Reports do not change §2 queue transitions, admission, focus or capacity.
- Menu and Conversations share one Reports view, exact result-SHA content, close-time pagination and SHA-bound decisions. Make tasks validates proposals and creation prerequisites before reserving creation, then creates linked held drafts through existing §1 claims. Unknown writes retain the report and known child numbers.
- The initial implementation was rebased on `9683b9ae586ca17a7d2d51977cf47b621b522402`, preserving the shared Conversations changes. Windows captures and the debug binary use source `f22d7b24758e9e1431f612c21a39d51a2123ba13`.

## Resumed candidate

- Rebased onto `dbdcb74ec606acc95ec9c543b54d380b54e99a7a`, which includes the merged [Mac evidence #1853](../1853/README.md). The current §7 screenshot policy derives README shots and design hashes at release; this branch retains the new Reports source in the file list and commits no generated hashes or README shots.
- Preserved current main's Conversations/history support, runtime context, publish-once instructions and owner-answer approval fences while resolving conflicts.
- Reverified all eight after-frame SHA-256 values against their manifests. Both platforms used source `f22d7b24758e9e1431f612c21a39d51a2123ba13`; the Reports layout and fixture remain unchanged. The resumed correction changes only an error notice; native frames do not qualify the exact final binary or current main's unrelated UI changes.
- Read-only Astra review traced authority, exact-SHA reads, claim/reservation ordering and UI/IPC connections. It found an omitted known issue number after uncertain draft creation. `node tests/reports.test.cjs` failed before with `known issue survives an unknown held-state readback`, then passed after the card displayed `Unverified draft: #21`. Confirmed held receipts remain separate; the report stays visible and claim/permission guards remain.
- `python scripts/check.test.py` passed after the rebase, including ordinary section citations and current main's selected-answer, trust, revision and attempt fences. Final build/browser/Rust/gate observations are in the result comment.

## Initial attempt verification

| Check | Observation | Scope |
|---|---|---|
| `cargo test --locked --bin taskq reports` | 20 passed, 1 ignored | Reports model, native board adapters, worker briefs, IPC and snapshot selection; ignored process fixture is not a pass |
| `node tests/reports.test.cjs` | Passed on final source | Source-bound receipts, read, Later, Drop, held creation dispatch, stale/unknown/partial outcomes, aliases and stable cursor |
| `node tests/reports.browser.cjs` | Passed 16 cells and 2 native-fixture bridge cases on final source | Real bundles with invented IPC; both surfaces, dark/light, 350/940 px, 16/20 px; separate from native acceptance |
| All `tests/*.test.cjs` | Passed after rebase and reader correction | Existing component, effect-inventory, source-hash and IPC guards retained; the subsequent failed-refresh correction reran the affected Reports checks |
| `python3 scripts/check.test.py` and `python3 scripts/check.py --rules --base bbeeae9af4acff3a4148def94e054ce5cf80f32c` | Passed | Natural section citation accepted; wrong section and unnamed rule changes still refused; rebased canonical gate also passed its rule check |
| `sh docs/shots/check.test.sh` | Passed | Missing/malformed manifests and unrecorded images still fail; stale sources remain detectable, including Reports |
| `python3 scripts/check.py --base 9683b9ae586ca17a7d2d51977cf47b621b522402` | Failed: 590 passed, 22 failed, 38 ignored; 621.468 s / 900 s | Rebased Windows worker candidate, before the final UI-only reader and failed-refresh corrections; no retries or weakened guards |
| `npm.cmd run build` and `cargo build --locked` | Passed on final source | Debug artifact only; both complete generated HTML bundles were verified present in the resulting binary |
| `taskq tray --snapshot <isolated-fixture> --route reports` and `--route reports-conversations` | Passed separately on final source | Four Windows WebView2 captures, light/dark at 16 px; invented report receipts, no live board decisions |
| `TASKQ_SELFTEST_OFFLINE=1 taskq selftest` | 99 golden traces and formats passed; 9 telemetry observations committed | Isolated debug artifact/private store; board unknown because offline; running writer unused |
| Immutable GitHub contents read at the rebase SHA | Returned blob `9070e94da7343befcf927422d5e41a3a4d48132b`, matching local Git | Encoded nested `docs/testing.md` source read only; no live task creation qualification |

Of the 22 canonical-gate failures, 18 owner-operation fixtures were blocked by worker identity (host installation, policy, lifecycle, resources, tray capacity and Windows security restoration). Four remain unresolved: `profile_health_flags_project_status_older_than_two_refreshes` reported a two-minute boundary stale; `native_pipe_bounds_unread_writes_and_empty_reads` timed out; `older_tray_writer_hands_over_to_new_host` did not finish handover; `web_execution_requires_acknowledgment_and_bounds_error_reports` could not acknowledge telemetry flush. Later integration/offline/Node stages did not run in that gate. Separate focused checks above do not turn it green. No permissions, worker identity, budgets or assertions were weakened (R9, R12).

## Before and after

- The new component check first failed because Reports was absent; the implemented shared view then passed component and browser checks.
- The citation regression first rejected `section 7`; after the parser fix it passed without allowing `section 70` or a subsection to approve its parent.
- A rejected Make initially wrote a fourth report comment instead of preserving three. Moving shared prerequisite validation before the make-intent made the same assertion pass; Drop and immutable-claim protection remain intact.
- Duplicate board registrations initially caused two reads instead of one. The corrected shared view passed the alias check, kept Drop effective and removed cached actions for unregistered projects.
- A failed first refresh initially appeared as a healthy empty report list. The new regression failed before the correction and passed after retaining the unknown state and collection reason.
- The [first native Menu capture](windows-menu-before-fixture-fix.png) showed Reports collapsed because the fixture lacked its pushed-state argument. Corrected invocation now opens the report in all four final native frames.
- The [first native Conversations reader](windows-conversations-before-scroll-fix.png) extended below the fixed footer. A corrected browser check targeting `.status-footer` failed before the CSS fix, then passed with wheel access to all three actions and restoration of Conversations after collapse. The earlier generic `footer` selector had falsely passed against a nested footer.
- Read-only review accepted the reservation/race, discovery, pagination, receipt-preservation, pending-open and prerequisite fixes. Review does not substitute for executable or native evidence.

## Native evidence and limits

[Windows manifest](windows-manifest.json) pins the source, fixture, binary and image hashes. Debug binary SHA-256: `f24aed3468f611b9415b00667bf5e9555df5131dbeb3666455c36f172d63657a`.

- Menu: [dark](windows-menu-dark.png), [light](windows-menu-light.png), captured at the start of the reader; actions are reachable by scrolling.
- Conversations: [dark](windows-conversations-dark.png), [light](windows-conversations-light.png), scrolled to show Make tasks, Later and Drop above the fixed footer.

These are Windows WebView2 observations on Windows NT 10.0.26300.0, with invented fixture data. The fixture's `mac` project/host text does not indicate macOS execution. Browser version and DPI were not collected. Other font cells remain untested. Two before frames, four Windows after frames and the four [macOS frames](../1853/README.md) use all ten Reports review-set slots.

[Mac evidence #1853](../1853/README.md) merged as `dbdcb74ec606acc95ec9c543b54d380b54e99a7a` with green merge-result CI. It records WKWebView light/dark frames, scrolled Menu actions and the standard screenshot update (failed stale check before; passed after); README shots were not committed. Mac display scale and native collapse/restore remain unmeasured. Reports implementation is [PR #1852](https://github.com/taskqorg/taskq/pull/1852); its own exact candidate CI and merge-result gate remain separate. No installed-host acceptance, deployment or completed #1826 publication is claimed (R12, §7).
