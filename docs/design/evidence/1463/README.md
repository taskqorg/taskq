# #1463 navigation and controls

Implemented source, awaiting A8 owner visual review. Not merged, released or
deployed. Native qualification and the merge gate are incomplete (R12).
Source blobs are pinned in the artifact receipts; UI before evidence: `0804397`.
Candidate rebased on `58744e1`; its M2 one-time note and publication fix are retained.
[Seven actual-size images](screenshots.md) · [source/artifact receipts](artifacts.json).

## Change and handoff

R13: the first commit amends the foundation and Product pointer before
implementation: M3/M4 fold into #1463–#1467, M2 runs in parallel, M5 follows.
This implements that approved decision; no authority or lifecycle rule changes.

- `app-shell.js/css` supplies the Preact shell, Signals frame and Wouter memory
  route, fixed Back/title/actions/window slots and surface tokens. Existing route
  templates use the platform parser and Preact reconciliation. M5 still owns
  retirement of obsolete shell mechanisms; this is not a claim of a completed
  component rewrite of every legacy content template.
- Public project/answer/resource JavaScript bundles are byte-identical to main.
  Project markup stays in a DOM island; answer/resource islands retain their
  existing executors and exclusive descendant ownership. The public CSS and its
  reference/provenance receipts change only for approved target/contrast repairs.
- Tray navigation retains its controller ports and native `navigation` setting.
  Workspace uses `workspace_navigation` in the same native settings store, with
  its own Messages root and Back stack. Both validate the bounded schema and
  explain removed destinations. No browser task store or mutation replay exists.
- #1464/#1465 consume the shared shell for answers/requests and reviews. Those
  operations still use their existing tray paths until moved by those tasks.
  #1466/#1467 own panels and messages/archive. Messages, Work sessions and Agent
  settings in the new shell explicitly explain their current unavailability;
  no TaskQ conversation storage was restored.
- The new workspace bridge currently accepts readiness, view-state persistence,
  Close and telemetry only. Later operation handlers must retain their existing
  typed IDs, authority checks and readback; this is not a generic operation bridge.

Window Close, route Back and emergency review remain separate. No running host,
queue, grant, capacity, task attempt or native agent session was changed.

## Acceptance observations

| Item | Evidence and limit |
|---|---|
| A1 / TW1 | One fixed title/bar on all supported route families; reserved leading root slot, breadcrumbs for projects/settings, logical-parent Back and explicit unavailable destinations. Workspace never returns to the tray through Back. |
| A2 / V05–V07 | Existing reload/filter/scroll/disclosure suite passes. The new shell suite proves invoking-control return, surviving heading fallback, heading focus on first entry/hydrated child, stable header DOM, keyboard Back, and independent workspace reload/Back. Native settings round-trip and size/schema refusal assertions pass in the Rust gate. |
| A3 / G01 | Baseline Back y: 45 → −4328 CSS px (4373 drift). Candidate: 4 → 4 (0 drift; limit 1). Same real-bundle browser boundary before/after. |
| A3 / G02 | At 320 CSS px / 40 px text, baseline Close right edge 372.69; candidate 312. Back is x=8, width=32, y=4, height=63. Both essential controls remain inside the viewport. A 210-character project name at the same size first exhausted the content area; the two-line title and single-line linked breadcrumbs now retain usable content and the resize control. Full route names remain in text and title attributes. |
| A4 | Shell suite measures ≥32×32 fine / ≥44×44 coarse enabled targets across 26 tray route/pointer cells; checks names, focusability, visible focus and nonoverlap. Existing independent link/keyboard checks remain. Workspace controls now have the existing 5 px option gap: its new check failed at 0 before and passes after. Native full keyboard/control painting is unqualified. |
| A5 / TW8 | One pressed/current/focus treatment, preserving native checkboxes and source semantics. All tasks is pressed only for unrestricted scope; selected state capsules remain independent OR toggles. Existing font/notification/window groups use the same treatment. Mac-only controls remain platform-unqualified. |
| A6 | [304 text cells](text-checks.json): 19 route variants × two themes × four fonts (14/16/18/20) × two widths (320/360). Zero measured type, horizontal-overflow, active-text contrast or enabled input-edge gaps. Titles compute to 1.1b/600; text ≥4.5:1 and measured essential input edges ≥3:1. Tray/workspace declare distinct spacing tokens. No spatial animation was added. This finite fixture set excludes disabled/native control painting and is not a WCAG or native acceptance claim. |
| A7 | Existing component, answer, resource, filter and link checks pass. IDs, original operation payloads/receipts, active-work guards and window/emergency distinctions remain. Public JS byte equality and asset hashes pass. |
| A8 | Seven images, source hashes and whole-route text checks retained. Agent inspection is complete; owner actual-size acceptance is pending. WebView2 evidence is limited to the fixture project route. WKWebView, installed layout and the actual new native workspace path remain unknown. |

The route ledger is Home, Projects, Project, filtered Tasks, global All tasks,
Attention, Settings, Appearance, Activity history, Notifications, Agents,
Services, About, Stop, Recovery, Work mode, workspace Messages, workspace Work
sessions and workspace Agent settings. The last two are explained unavailable
destinations, not newly implemented backends.

## Verification and failures

- `python scripts/check.py --base 58744e1`: **failed**, full selection on the
  rebased implementation, 50.375 s / 900 s; Rust 421 passed, 20 failed, 35 ignored. Explicit worker/host
  authority and Windows restore refusals are **sandbox-blocked**, never PASS.
  Other assertions remain unqualified, including
  `service::tests::a_waiting_publication_reads_nothing_twice_and_only_its_host_polls`
  (unexpected issue reads). No relevant service/host/lifecycle source changed;
  that is not a baseline reproduction or a passing gate. [Failure list](checks.md).
  The gate stopped before its offline selftest and Node stages. The pre-rebase
  run on `0804397` also had 421/20/35 (53.859 s); the final run was required by
  the updated main candidate. Both retain the same 20 failed checks.
- `cargo +stable run --locked -- selftest`, isolated install with
  `TASKQ_SELFTEST_OFFLINE=1`: 92 golden traces, formats and staged telemetry pass;
  board remains unknown because no board read was made.
- Every `tests/*.test.cjs` component/IPC check passes, including asset provenance.
  `docs/shots/check.test.sh`, `.github/release-tag.test.sh` and
  `.github/release-check.test.py` pass. The new shell-source fixture first read
  stale CSS as fresh, then passed after expanding the source list. The actual
  README screenshot manifest is still stale and deliberately not re-approved.
- `node tests/shell-navigation.browser.cjs`, `tray-answer.browser.cjs`,
  `tray-state-filter.browser.cjs`, `tray-view-state.browser.cjs`,
  `tray-window.browser.cjs`, `tray-resources.browser.cjs`,
  `tray-stop.browser.cjs`, `tray-recovery.browser.cjs` and
  `project-blocks.browser.cjs` pass. The last retains 64 parent/browser/tray
  comparisons, wrapping, independent links and provider checks.
- One read-only Astra review found initial focus on BODY and a sticky workspace
  construction error. Both were repaired; the reviewer reran the focused bundle
  check and confirmed first/hydrated focus and explicit error-clear handling.
  A later focused review found no blocking gap in the intentional workspace focus
  handoff guard. Its new unit assertion failed before and passes after; the
  original 15 ms, 300 ms and 5 s popover protections remain. Actual native
  focus-event ordering is still unknown.
- The All tasks assertion failed on unchanged main as well: it expected no
  pressed control after clearing restrictions. Repair requires All tasks pressed
  and every state chip unpressed (#1455), retaining all original risk protection.
  Reference 28 px/quiet-text tokens were aligned with approved V09/V24; parent
  comparisons and every checksum assertion remain. Both the direct CSS receipt
  and its transitive finding-composer receipt were refreshed.
- R8 size check first failed after the owner steer pushed the contract above
  60 KiB. Shortening its Product pointer restored 61,429 bytes and 298 lines;
  all nine `core::tests` pass with the original 60 KiB / 300-line bounds retained.
- Native command: `cargo +stable run --locked -- tray --snapshot .taskq/evidence/1463/native/current-shots --route project`,
  isolated install over `docs/shots/fixture.sh`: exit 0, Windows WebView2 project
  render captured. Direct execution first exited `0xC0000135` (loader unavailable
  outside Cargo). This proves a development render, not relocated/installed
  layout or native workspace ownership/raising. The capture was repeated for
  the rebased implementation; its source and binary have their own receipts.

No test assertion was deleted or weakened. No worker environment, model/runtime
policy or permissions were relaxed. Later source changes were checked at their
affected boundary; the failing canonical gate remains a gate failure.

## Owner scope and UI instrument

[Owner steer 6314418072](https://github.com/taskqorg/taskq/issues/1463#issuecomment-6314418072)
keeps performance observation in existing CoreQ telemetry. No benchmark scripts,
performance tests or latency/memory ceilings are added to this or later UI PRs.
The owning foundation and Product pointer were amended before applying the steer.

`text-check.cjs` is a UI acceptance instrument, not a new CI gate. It emits only
route text, computed type, overflow and contrast observations; it self-checks the
contrast calculation. Its first expanded route run stopped at an incorrect
work-mode fixture entry; the corrected entry uses the project launcher. Native
and browser results stay separate under R12.

## Remaining acceptance

Owner A8/V38 actual-size review is required before adopting the visual references.
Then refresh `docs/shots/manifest.json` through the qualified screenshot path;
the expanded watcher includes the new shell/workspace sources. Do not hand-edit
old image hashes to imply a fresh capture. Green merge-result CI and the missing
native evidence remain required; neither source delivery nor visual approval
authorizes rollout (§ 7, R12).
