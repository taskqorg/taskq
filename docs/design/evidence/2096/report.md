# Settings correction: #2096

The twelve Settings destinations retain their controls and authority. The candidate fixes the Nov 1 F05/F06 and Settings portion of F10. The task's [owner correction and packet](https://github.com/taskqorg/taskq/issues/2096) govern the presentation changes; the frozen reference in the implementation brief remains unchanged.

## Applied requirements

R13: Product, M1.1/M1.2, M8/M12, TW1/TW2 and R28/R30 geometry change before implementation. R9 and §7 authority, runtime choices, grants, capacity and publication policies remain unchanged. R12 separates browser/source evidence from installed native acceptance.

| Destination | Result and retained behavior |
|---|---|
| Appearance | One active opacity pane; independent 60–100 and 0–100 values, Off, font, icon and growth controls; native readback/error path retained. |
| Notifications | Compact labelled selectors, one mode legend, effective project overrides, exact OS refusal and deliberate Test controls. No test notification is sent by browsing. |
| Activity history | Existing graph, period/custom range, lower bounds, missing coverage and source details. |
| Hosts and resources | Existing comparable runtime/total table, roles/resources, freshness and unknown/partial reasons. Root shows bounded scope. |
| Work mode | Current admission counts and exceptions; complete project intake/access readback in detail; direct project-admission and host-limit reviews. |
| Agents and access | Client versions/accounts once; comparable project/runtime access, exact requested/granted/applied/refused facts and remote missing-source reasons. |
| Projects | Direct overview, registered board and local folder shortcuts; exceptions visible; intake intent/readback and review remain in detail. |
| Delivery | Presets explained without policy edits; exact-candidate authority retained; Revision 0 is secondary. GitLab direct retains its fast-forward MR qualification. |
| Services and support | One host process observation, exceptions first, complete project observations/logs/incidents/actions on demand; AgentQ stays distinct. |
| Emergency stop | Existing shared review, named targets, separate Drain/Full stop confirmations and refusal reasons; project diagnostics remain expandable. |
| Recovery | Current hold and host-service observation; direct selected-local-task recovery review; unknown effects remain held. |
| About and updates | Exact installed/running/available versions, update/rollback review and failure/readback remain. |

## Failure before, pass after

- F05: on clean main `4bbce5e2`, the browser reproduction found Settings rows computed as `display:flex`. At 320 px/18 px the Work mode label overlaps the eleven-project summary. The component-owned grid now places label and bounded summary in separate rows. The obsolete selector that expected `data-a=work` is removed.
- F06: the component check failed because Background only still rendered `menu-opacity`. It now passes with exactly one active range. Existing keyboard/IPC, independent values, background fills, foreground alpha, contrast and error/readback protection remain.
- F10: the same clean baseline's Work mode contains only a review launcher. The candidate shows current admission/hold state, scoped reviews and complete details; opening/Back submits no operation.
- Expanded disclosure checks found the rotating arrow spilling from an 8 px track. The shared square slot fixes Settings and existing operation reviews without clipping text.

## Verification

- `tests/settings-content.browser.cjs`: Pass, 104 cells: Settings root plus twelve destinations × 320/560 px × 18/20 px × both themes, collapsed and expanded. Includes opacity switching/rollback snapshot, notification IPC/overrides, access, all destinations and review/Back focus.
- `tests/workspace-operations.browser.cjs`: Pass, 32 review cells × collapsed/expanded; no clipping, horizontal scroll or effect on entry. Existing exact Full stop, single-use/unknown receipt and draft preservation guards remain. Its fixture now supplies the required display model and run mode; no assertion was removed.
- `tests/tray-background.browser.cjs` and `tests/tray-opacity.browser.cjs`: Pass. The obsolete inactive-disabled assertion changes to inactive-absent; range, saved values, fill/alpha, mutation, error and isolated-window protection remain.
- `tests/conversations-centre.browser.cjs` and `tests/conversations-render.browser.cjs`: Pass, including common-reader navigation and the existing 10,000-record browser model.
- All 35 Node component scripts and four artifact/release scripts: Pass. Exact commands are in the attached checks.json.
- Offline `cargo +stable run --locked -- selftest`: Pass, 113 golden traces and eight committed observations in an isolated temporary store; board/tool probes intentionally remain unknown.
- Selected `python3 scripts/check.py --base 4bbce5e26d6cab9f113c055bf15cc33516162edc`: sandbox-blocked after 161.445 seconds, not Pass. Remaining Node/browser/selftest checks ran separately after the abort. The gate's protected integration test is sandbox-blocked, never Pass: `sandbox-exec: sandbox_apply: Operation not permitted`. It passed 808 unit tests and 21 integration tests before that refusal; 37 unit tests and one integration helper were ignored.
- `python3 scripts/check.py --lint`: Pass, 0 errors, 240 lines, 48,484 bytes. The lint uses the checker's immutable SpecQ pin; rule delivery uses the task's injected SpecQ revision. The canonical run reported the absent local lint cache; the same pinned lint then ran separately.

## Evidence and limits

The attached ZIP has one index, the invented shared fixture, 104-cell matrix, checks, source/build/frame hashes and exactly ten attributable screens: three clean-baseline before screens and seven candidate after screens. All ten were visually inspected. Source/browser evidence does not claim a deployed or independently accepted native result.

`tests/shell-navigation.browser.cjs` still fails its 320 px/40 px case (`G02/V23: essential Back/Close clipped`). The same unchanged check fails on a separately built clean `4bbce5e2`; this baseline limit is outside the task's 18/20 px matrix and is retained explicitly.

No live installation, restart, maintenance, grant, real PM/owner message or native qualification was performed. Native Mac/Windows rendering, physical accessibility and the independent owner-chat acceptance remain Not tested. Publication still requires green merge-result CI under §7/R12.
