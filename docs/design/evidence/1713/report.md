# Settings navigation — #1713

R13 Product and TW1–TW2 keep Settings inner pages in Settings and keep explicit
operation reviews in the workspace. This change covers navigation and route titles.
It leaves operation review content, authority and native operation handling intact.

## Reproduction and acceptance

Base: `66641866f8cf6844175e8fd7aa70b40f5a692aa1`.
The extended `node tests/settings-content.browser.cjs` failed before the source fix:
`#1713: inner title equals list label`, actual `Settings`, expected `Work mode`.
The same instrument passes after the fix (R12, § 7 step 4).

| Acceptance | Observation | Result |
|---|---|---|
| A1 | Work mode opens an inner Settings page; Back returns to the list and restores its link's focus. | Pass, browser |
| A2 | Recovery opens an inner Settings page; Back returns to the list and restores its link's focus. | Pass, browser |
| A3 | Agents and access and Services and support use their full list labels as titles. About and updates matches too. | Pass, browser |
| A4 | Opening and refreshing the inner pages sends no operation or workspace handoff. Explicit review links retain their original operation/target. | Pass, browser |

The existing bundled Settings instrument retains widget, sign-in, unknown/access,
host service and Appearance protection. Its added cases run at 320 px in both
themes, with and without host capacity. An absent capacity projection retains the
admission review destination; a present one retains the resources review.

After images show the actual bundled Settings surface with invented data, 16 px,
320×900, dark theme, Windows headless Chromium. They are browser evidence, not
native window or live operation proof:

- [Work mode](settings-work-320-dark.png)
- [Recovery](settings-recovery-320-dark.png)

Before native evidence: [#1703 report](../../review-evidence/native-1703-2026-10-25/report.md),
screens 04, 06, 08 and 09.

## Additional checks and limits

`node tests/window-chrome.browser.cjs` passes: Settings chrome, Home/Back and
independent menu geometry/navigation remain protected. The existing
`node tests/tray-recovery.browser.cjs` passes its selection, evidence, unknown and
same-payload reconciliation checks. Neither instrument performs native effects.

Two broader optional instruments fail on both this change and the base source:

- `node tests/shell-navigation.browser.cjs`: `G02/V23: essential Back/Close clipped at 320px / 200% text`.
- `node tests/workspace-operations.browser.cjs`: `Cannot read properties of undefined (reading 'all')`; its snapshot lacks the required display model.

The base comparison bundles the exact base `src/tray-app.js` with unchanged
dependencies and remaining source, then runs the same instruments and fixtures.
These pre-existing failures are not claimed as passes. No shell layout or obsolete
fixture repair is included in this Settings navigation task.

Native Settings clicks/window raising, macOS rendering and live operation effects
are not tested. Publication requires green merge-result CI (§ 7, R12); worker
handoff is not merge, release or deployment acceptance.

The first full local gate failed after 493.297 s: 523 Rust tests passed,
20 failed, 36 were ignored. The added root-contract sentence exceeded its exact
60 KiB limit by 66 bytes. It was removed: the existing Product link already binds
the accepted P-W2 placement, and TW1–TW2 now clarify these routes without growing
the root contract. The size assertion is unchanged.
`cargo +stable test --locked --bin taskq core::tests::every_table_row_has_a_golden_trace -- --exact`
passes after the correction.

Eighteen failures exercise host/operator capabilities refused by the inherited
worker identity, including `a worker cannot change host policy`, `a worker cannot
install the host service` and `Windows security restore requires an unrestricted
operator`. These remain sandbox-blocked; worker restrictions are retained.
The native named-pipe deadline fixture also failed under the full run; its exact
filtered run passes in 0.20 s. This does not erase the full-run failure or qualify
native acceptance. CI must qualify the merge result outside this worker context.

The separately run Node component suite initially found the expected stale
`task-workspace.sources.json` hash for the changed tray source. The owning
`node tests/task-workspace.test.cjs --write` command updates that hash; the original
source-provenance assertion remains intact and passes afterward.

All 25 `tests/*.test.cjs` files then pass, each invoked with Node. README shot
freshness and its fixture checks, release-tag fixtures and exact-source release
checks pass. An isolated `cargo +stable run --locked -- selftest` passes 99 golden
traces, format checks and its private staged telemetry capture; board access is
explicitly unknown in offline mode. The native README snapshot refresh passed on
Windows WebView2 and pins the changed UI blob without changing operation code.

## Merge-conflict continuation

Attempt `6400263715` rebases the existing implementation onto
`914d603cee420743f1e8b1d771e9d27d09ecaeb5` after the merge queue removed PR #1717
with `merge_conflict`. The conflict is in the generated README icon and screenshot
manifest: main refreshed them for #1711 while this task refreshed them for #1713.
The Settings implementation and regression check rebase without conflicts.

The rebased Settings browser check passes A1–A4 in both themes at 320 px, with
and without capacity. Window chrome and the existing Recovery operation browser
check also pass. These observations retain the original no-effect-on-entry,
explicit-operation-review, Back-focus and operation-fencing protection.

`sh docs/shots/update.sh` regenerates the conflicting files from combined source
`847b5b0a5276ba8169a6d1f0754d5fae494dc6c3` in an isolated Windows installation;
`sh docs/shots/check.sh` passes afterward. The manifest records the Windows
WebView2 renderer, binary digest and both tasks' current UI source blobs. This
resolves the generated-file conflict without changing either implementation.


## macOS merge-conflict continuation

Attempt `6400570491` rebases the same PR onto
`084a4526596e5eea439fec2530698df35cfa6ff6`. Settings source and its regression
check merge unchanged; the conflicts are generated source provenance and the
README screenshot manifest. The source manifest is recomputed with its existing
`node tests/task-workspace.test.cjs --write` owner command; the source-provenance
assertion remains intact and passes.

The current main `src/tray-app.js`, bundled in an isolated temporary copy with
the unchanged regression check, reproduces `Settings` instead of `Work mode`.
The combined candidate passes the same Settings check in light/dark at 320 px,
with and without capacity, including all A1–A4, refresh retention, Back focus and
explicit review destinations. Window chrome and recovery browser checks pass.

The two linked after images are refreshed from the combined candidate on macOS,
Chromium headless, 16 px, 320×900, dark, invented fixture data. Both were visually
inspected: Settings breadcrumb, inner title, Back, Settings window controls and
explicit review link are visible without clipping. These are browser observations;
native Settings clicks and window raising remain untested (R12).

`sh docs/shots/update.sh` passes with an isolated fixture installation and real
WKWebView snapshots on macOS 27.0.1 arm64. `sh docs/shots/check.sh` passes with
the refreshed combined-source manifest (source
`9a795c0fe1317181e7e0baedab14d143c244e1e2`, release binary SHA-256
`c29916fab9edfcd64a2a023ce2b55755edc83ed1eb7e2a2201290d26e808e3db`).
The README project image and summary icon were visually inspected after export;
these native static fixtures do not prove Settings clicks/window lifetime.

`target/release/taskq selftest` passes in an isolated offline installation:
99 golden traces, formats and nine staged capture observations. The board is
explicitly unknown; the running installation is untouched. Tooling: Node v22.18.0,
Cargo 1.98.1, warm seeded debug cache; release dependencies built locally.


The frozen combined-source gate
`python3 scripts/check.py --base 084a4526596e5eea439fec2530698df35cfa6ff6`
fails in 131.320 s (budget 900 s): 537 Rust tests pass, 17 fail, 32 are ignored.
Failures are host installation, capacity/resource policy, lifecycle and the tray
capacity/pause fixture. Direct refusals include `a worker cannot change host
policy`, `a worker cannot install the host service`, `a worker cannot restart the
host service or request Stop` and `a worker cannot dispatch host Stop`.
These operator capabilities are sandbox-blocked by the inherited worker identity;
their secondary expected-receipt/phase assertions also fail and remain unqualified.
No worker guard, host policy, permission or test expectation was changed. Rust
integration and the gate's subsequent selftest/Node stages were not reached.

All 25 `tests/*.test.cjs` files pass separately on the same bundled source, as do
the screenshot fixture, release-tag fixture and exact-source release checks
(28 commands). The separately run offline release-binary selftest above passes.
The Settings browser regression is unchanged from the before/after observation;
its original no-effect-entry and explicit-review fencing protection is retained.
Publication still requires green merge-result CI (R12, § 7); native Settings
clicks and live operation effects remain unknown. No merge, release or install
was performed.
