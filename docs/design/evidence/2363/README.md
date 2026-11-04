# Native UI follow-up: incomplete candidate

Source: [owner request and screenshot](https://github.com/taskqorg/taskq/issues/2363). Applies R6/R7/R8/R12/R13 and section 7. Product and linked UI decisions changed first under R13. No host policy, active attempt, canonical task identity or task destination changes.

## Implemented boundary

A2-A4 and A6-A7: wrapped counters precede the toolbar; one leading List button uses pressed=flat; one monochrome Antigravity path; task identifiers and status versions share the literal-copy widget. Home, grouped rows, pins, Conversations sidebar, task/linked/result headers and result/event readers reuse it. Clipboard success/failure preserves routing and editing focus. Row navigation retains Enter/Space; Copy has its own keyboard/touch target.

The source fix for one A5 feedback path redraws/refits on width reflow, not on height-only native acknowledgements. Content/font changes and intentional manual dimensions retain the existing fitting path. No timer, request suppression or native resize disabling was added.

## Failure before and pass after

Base: `699ad95d6bb04bdc8bd58662a50958b6377a68e4`.

- `native-ui-followup.browser.cjs`: the immutable base fails pressed-state acceptance (`true` versus `false` for grouped mode); the candidate passes layout, grouping/view/search preservation, monochrome icon, exact identifier/version clipboard contents, failure feedback, keyboard/touch and Conversations consumers in both themes.
- `resize-feedback.browser.cjs`: four height changes plus explicit resize acknowledgements produced six size requests on the base and zero on the candidate. Width and content/font changes still fit. This is browser adapter evidence, not proof of the owner's native Mac oscillation or CPU cause.
- `task-navigation.browser.cjs`, `task-dashboard.browser.cjs`, `tray-task-list.browser.cjs`, `owner-ui-review.browser.cjs`, `window-chrome.browser.cjs`, `tray-home-header.browser.cjs`, `project-blocks.test.cjs`, `task-workspace.test.cjs`, `tray-running.test.cjs` and `tray-web.test.cjs` passed the affected checks. Changed assertions retain exact routing, keyboard access, pin order, health warnings and source identity while admitting the requested Copy control and new display labels.
- `tray-refresh.browser.cjs` initially waited for the removed Home heading. Its assertion now follows the existing no-heading Product decision; one settled fit and persistent-unknown checks pass unchanged.
- `reports.browser.cjs` times out on `[data-report=open][data-n="12"]` on both the immutable base and candidate. Its earlier report-row selector is not current; its wider instrument remains unqualified. Existing result/source component protections remain in the canonical gate.

## Native observations and outstanding work

Windows installed baseline `4.6.76` completed four isolated Home WebView2 snapshots (dark/light, 16/20) in 4710 ms, exit 0. They do not capture native frame composition or prove resting stability. Export contains one coalesced `tray.window.resize` observation; coalescing prevents interpreting it as a native event count. CPU was not measured. No installed service was restarted or replaced.

A1 remains unimplemented. Simply disabling whole-window transparency was removed from the candidate: it risks breaking existing background-only opacity. Tao 0.37.1 sets a transparent NSWindow's background to clear; the native Mac remedy must preserve content appearance preferences while restoring an ordinary native titlebar.

A5 native reproduction, actual resize-event and CPU before/after measurement, causal attribution and native stability after content/wrapping/titlebar changes remain unknown. A8 browser clipboard/layout proof passes; native frame, native clipboard and installed Mac behavior remain open. Automatic review rejected a proposed low-level native-event-hook probe as blocked by policy; the ordinary isolated snapshot and existing profile export were used instead, without bypassing the refusal.

The complete task is not ready for result/acceptance. [Mac blocker #2375](https://github.com/taskqorg/taskq/issues/2375) was filed held and read back under R2 and section 7. It returns native measurements and a minimal correction patch as data against frozen implementation `1b2371899d836213a0b4dce30196eb613b568333`; the parent retains all A1-A8 and final implementation/publication. The parent parks on that blocker so the service can start it. UI acceptance and merge-result CI remain separate under R12.

## Candidate gate

Rebased once onto `026db2524d6d5d7b34452d4c8d0fbf17073df7b9`. `python scripts/check.py --base HEAD^` failed after 7.860 s at the rule gate: task scope does not name `M3`, `M13`, `Product`. No Rust suite, offline selftest or later canonical checks ran. The gate ran at `eff7fd050d5ae008425b36f83eebaea993df78ed`; subsequent changes only record evidence and opt-in browser capture. That initial refusal was preserved without bypassing the gate or editing the started issue. Accepted answer `6636967640` now records M3/M13/Product. `python scripts/check.py --rules --base 026db2524d6d5d7b34452d4c8d0fbf17073df7b9` passes against that owner record. The canonical gate resumed at `721f70105a10aefcdda37b5568d3c0c5166bf3b9` with `python scripts/check.py --base 026db2524d6d5d7b34452d4c8d0fbf17073df7b9`. It timed out after 900.031 s during the cold Windows Rust build (cargo allowance 887.047 s); no Rust test result was produced. Compiler output contains warnings, not compilation errors. Offline selftest and later canonical Node/browser checks did not run. No owned compiler remained afterward. Contract lint also remains unknown locally because its pinned script checkout is absent; CI owns that check. The budget was not extended and the full gate was not retried. Native acceptance remains incomplete.

Browser pictures: [dark Home](home-dark.png), [light Home](home-light.png), [Conversations](conversations.png). These show content rendering, not native titlebars or movement. They were captured after the one rebase. No claim of CI, native qualification, publication or deployment.
