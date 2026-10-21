# #1463 navigation and controls

Implemented source. The owner accepted the seven-image set, including the four
heading-focus and breadcrumb corrections in [answer 6315388620](https://github.com/taskqorg/taskq/issues/1463#issuecomment-6315388620).
Not merged, released or deployed. Native qualification and the merge gate remain incomplete (R12).
Source blobs are pinned in the artifact receipts; UI before evidence: `0804397`.
Candidate rebased on `a4605ba`; the M2 note, publication fix and new-task intake change are retained.
[Seven actual-size images](screenshots.md) · [source/artifact receipts](artifacts.json).

## Resumed capture and notification repair

The branch incorporates `00a8a4f`, including #1488's npm setup for README shots.
The manifest check reproduced exit 2 before capture. The first resumed macOS
[capture 39602159027](https://github.com/taskqorg/taskq/actions/runs/39602159027)
at `a2ed8b6c180e0ec4911fcadf8266695e25340600` built successfully, then failed
its unchanged native overflow check: Banner and Banner with sound exceeded
their buttons by 2 CSS px in both themes.

V23/V26 repair: notification toggle groups now use content-based flex bases
instead of the shared 38 px basis, retaining wrapping, targets and selection.
The added case in `tests/shell-navigation.browser.cjs` failed before at 18 px
(Inherit: 2 px overflow; Banner and Banner with sound: 7 px), then passed after
for host and project controls at 14/16/18/20/40 px. Every existing assertion
remains. This fixes implementation against the existing rules; no rule or
protection changed. Chromium and Windows Playwright WebKit probes agree; the
latter is a browser observation, not WKWebView qualification.

`python scripts/check.py --base a2ed8b6` passes the affected web selection in
65.875 s / 900 s: nine Rust core checks, every component/IPC check, release and
manifest-script protection, and the repaired shell browser check. The separate
view-state/filter/window checks pass. Whole-route text checks again report
304 cells with zero measured gaps. Isolated offline selftest passes 92 golden
traces, formats and staged telemetry; the board is unknown (not read).

The fresh full local gate at `a2ed8b6` failed: 424 passed, 21 failed, 35 ignored,
84.640 s / 900 s. It retains the earlier 20 failures and adds a native-pipe peer
accept deadline failure; the R8 size check passes. Explicit host/policy/security
refusals are sandbox-blocked. The remaining assertions and native-pipe timing
stay unqualified. No worker authority was removed and no failure was retried
into a passing claim. The notification repair changes no Rust behavior.

Windows workspace qualification was attempted with an isolated fixture tray.
Computer Use returned `GetCursorPos failed: Access is denied. (0x80070005)`;
one fresh read returned `IGraphicsCaptureItemInterop.CreateForMonitor failed:
Could not capture the given monitor. (0x80070057)`. No workspace interaction was
proved. The fixture tray was stopped after verifying its recorded PID and
executable; the running installed tray/service and active attempts were untouched.

The repaired source also passed the separate native WebView2 project snapshot:
`cargo +stable run --locked -- tray --snapshot .taskq/evidence/1463/resume-native/shots --route project`
emitted two PNGs (dark/light, 16 px) and exited 0. Fixture paths were converted
to native Windows paths before capture; an earlier POSIX-path fixture emitted
no frames and does not count. Development binary SHA-256:
`b8953f11eec8e6131c2b20d86a5a6793d1e54dc03df78d4503e7d98c71662abb`.
This is project rendering only, not native workspace or installed qualification.

Replacement macOS [capture 39603193860](https://github.com/taskqorg/taskq/actions/runs/39603193860)
was dispatched on `a200eca` after the repair and remains queued. No matching
`readme-shots` artifact exists yet; the committed manifest still exits 2. Resume
only after reading the successful artifact's actual `sourceCommit` and matching
its UI blob IDs to the candidate, then applying the captured images and manifest.
Do not substitute browser output or rewrite old hashes. The original seven
review images remain unchanged and accepted. Native workspace and merge-result
CI gates remain open (R12; §7).

## Owner correction: heading focus and breadcrumbs

[Steer 6315125709](https://github.com/taskqorg/taskq/issues/1463#issuecomment-6315125709)
supersedes the earlier full-set answer. The rest of the set remains accepted.
Only G01-after, G02-after, workspace-root-after and webview2-project-after were
re-recorded; all three before-images are byte-identical. Compare the
[previous seven-image set](https://github.com/taskqorg/taskq/blob/0dd5589019061b4f0755709efe666d8327a8f262/docs/design/evidence/1463/screenshots.md).

R13/TW1 now specifies linked ancestors only: Projects for a project overview,
Projects › project for Tasks/Work mode, Settings for a settings category. The
current page appears once in the route title. Programmatic heading focus remains
for accessibility and route restoration, with no box; keyboard controls retain
`:focus-visible` rings. Before: `Projects › Alpha` repeated the current title and
the workspace heading computed `outline: auto`. After: parent-only route checks
and `outline: none` pass in both shells. Parent-link navigation, Back restoration,
keyboard focus, target/nonoverlap and G01/G02 protection remain. The old
current-page-link/count assertion was replaced under the owner-approved TW1
change; no unrelated guard was removed. The added filtered-route case caught a
misencoded separator, which was repaired as UTF-8 before passing.

`node tests/shell-navigation.browser.cjs`, the view-state/filter/window checks,
all component checks and the 304-cell text instrument pass after the corrections.
Windows WebView2 project evidence was refreshed with `cargo +stable run --locked -- tray --snapshot .taskq/evidence/1463/native/owner-revision-shots --route project`; workspace/WKWebView rendering
qualification remains unknown. Native click/raise is owner-observed per the UI
acceptance contract, not an additional click-proof gate.

`python scripts/check.py --base a4605ba` failed: 421 passed, 21 failed, 35 ignored,
91.047 s / 900 s. The additional failure was R8 size after the intake rebase.
The touched Product pointer was shortened; 61,433 bytes / 298 lines now satisfy
the unchanged bounds and all nine `core::tests` pass. The remaining 20 canonical
failures are unchanged. This is not a green full gate. The separator and pointer
repairs were checked at their affected boundaries; the full suite was not rerun.
Offline selftest again passes 92 traces/formats/staged telemetry, board unknown.

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
| A1 / TW1 | One fixed title/bar on all supported route families; reserved leading root slot, parent-only breadcrumbs for projects/settings, logical-parent Back and explicit unavailable destinations. Workspace never returns to the tray through Back. |
| A2 / V05–V07 | Existing reload/filter/scroll/disclosure suite passes. The new shell suite proves invoking-control return, surviving heading fallback, heading focus on first entry/hydrated child, stable header DOM, keyboard Back, and independent workspace reload/Back. Native settings round-trip and size/schema refusal assertions pass in the Rust gate. |
| A3 / G01 | Baseline Back y: 45 → −4328 CSS px (4373 drift). Candidate: 4 → 4 (0 drift; limit 1). Same real-bundle browser boundary before/after. |
| A3 / G02 | At 320 CSS px / 40 px text, baseline Close right edge 372.69; candidate 312. Back is x=8, width=32, y=4, height=63. Both essential controls remain inside the viewport. A 210-character project name at the same size first exhausted the content area; the two-line title and single-line linked breadcrumbs now retain usable content and the resize control. Full route names remain in text and title attributes. |
| A4 | Shell suite measures ≥32×32 fine / ≥44×44 coarse enabled targets across 26 tray route/pointer cells; checks names, focusability, visible focus and nonoverlap. Existing independent link/keyboard checks remain. Workspace controls now have the existing 5 px option gap: its new check failed at 0 before and passes after. Native full keyboard/control painting is unqualified. |
| A5 / TW8 | One pressed/current/focus treatment, preserving native checkboxes and source semantics. All tasks is pressed only for unrestricted scope; selected state capsules remain independent OR toggles. Existing font/notification/window groups use the same treatment. Mac-only controls remain platform-unqualified. |
| A6 | [304 text cells](text-checks.json): 19 route variants × two themes × four fonts (14/16/18/20) × two widths (320/360). Zero measured type, horizontal-overflow, active-text contrast or enabled input-edge gaps. Titles compute to 1.1b/600; text ≥4.5:1 and measured essential input edges ≥3:1. Tray/workspace declare distinct spacing tokens. No spatial animation was added. This finite fixture set excludes disabled/native control painting and is not a WCAG or native acceptance claim. |
| A7 | Existing component, answer, resource, filter and link checks pass. IDs, original operation payloads/receipts, active-work guards and window/emergency distinctions remain. Public JS byte equality and asset hashes pass. |
| A8 | Seven images, source hashes and whole-route text checks retained. The owner accepted all seven, including the four corrections (answer 6315388620). Historical WebView2 evidence is limited to the fixture project route. Installed layout and the actual new native workspace path remain unknown; see the resumed capture evidence above. |

The route ledger is Home, Projects, Project, filtered Tasks, global All tasks,
Attention, Settings, Appearance, Activity history, Notifications, Agents,
Services, About, Stop, Recovery, Work mode, workspace Messages, workspace Work
sessions and workspace Agent settings. The last two are explained unavailable
destinations, not newly implemented backends.

## Earlier implementation checks and failures

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

Owner A8/V38 actual-size review of the seven-image set is recorded in answer
6315388620. Refresh `docs/shots/manifest.json` through the qualified screenshot path;
the expanded watcher includes the new shell/workspace sources. Do not hand-edit
old image hashes to imply a fresh capture. Green merge-result CI and the missing
native evidence remain required; neither source delivery nor visual approval
authorizes rollout (§ 7, R12).
