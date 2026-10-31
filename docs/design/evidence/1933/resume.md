# Integrated Windows qualification: resumed checkpoint

Native qualification remains incomplete. This checkpoint resumes attempt 6536485525 after #1994; it does not supersede the [original failure](README.md) or qualify the interface. The [report](../../../research/1933.md) maps remaining acceptance and the owner's submission disposition. No implementation or rule changed (R13).

## Exact candidate and native observations

- Source: `cbeb8db913aa0908fbe9490e7de8404e7a306231`, TaskQ 4.6.50, branch `taskq-1933`. The prior evidence was preserved while rebasing onto this integrated source. All seven implementation/#1918 merge commits listed in the original checkpoint, plus #1994's merge, pass `git merge-base --is-ancestor <merge> HEAD` with exit 0. This is integration proof, separate from native acceptance (R8/R12).
- `cargo build --locked`: exit 0, 2m16s, 25 warnings. GNU debug executable SHA-256: `124f2a2303c9eaae4e9978f066225c7bcf2bc9f9f1d85f55f2d3f740acdf53e6`. This is a development build, not a staged release or installed-product qualification.
- Windows registry: 26H2, build 26300.9711, ProductName `Windows 10 Pro`. One primary display: 2560×1440, work area `(0,0,2560,1392)`. Application/system theme: dark. Accessibility TextScaleFactor: 140; AppliedDPI registry value: 96. Registry DPI alone does not qualify native per-monitor DPI or monitor transitions.
- The actual WebView2 capture process reports `Edg/156.0.4314.50`, protocol 1.3, JS `15.6.12.7`. Native renderer DPR: `1.399999976158142`; CSS screen 1829×1029, available 1829×995. The sampled body computes to `16px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`. This is a computed CSS stack, not glyph-ink calibration or proof that every control uses 16 px. See [menu readback](menu-native-readback.json) and [Conversations readback](conversations-native-readback.json).
- A bounded four-scene Home metadata probe independently reads the menu's actual `--size` and computed font at 16/20 px. Its heading uses native `Segoe UI Semibold` (`SegoeUI-Semibold`, not a custom web font), with a 22 px heading in the sampled 20 px case. [Font readback](font-native-readback.json) records the seven samples. Its first invocation waited behind the transcript test compilation and exhausted the metadata connection window, yielding zero samples despite four successful captures; that observation was preserved in scratch, then only this bounded metadata probe was repeated after the build lock released. No glyph-ink or other-DPI pass is claimed.
- A temporary loopback debugging endpoint read native metadata only; it injected no input or delivery and used no Chromium platform override. It ended with its disposable snapshot process. Samples include startup/transitional layouts; their node counts are not long-history bounds or performance measurements.
- Fixture: unchanged `docs/design/tray-fixture.sh`, three invented projects, no EXTRA projects. Its alpha/beta/gamma observations deliberately include incomplete/error/stale data. The generated records are not live host availability or exact-count proof. Conversations uses unchanged `tests/fixtures/conversation-shots.js`, whose provider transcript and effects are intercepted inventions. [Provenance](provenance.json) pins their bytes and generated fixture hashes.
- The unpacked packet's 194 payload entries again pass their manifest sizes and SHA-256. Its README, plan, acceptance, issue map, landing-page HTML and Q-A1–Q-A12 were read. The canonical UX30/capability specifications and both fixture source files are unchanged between the failing and resumed sources.

## Failure before, pass after

The original source `554552daf48d5b413918d54a8a6d0f88efba4020` produced 50 native menu captures and exited 1: bell `button 6 by 3 px`. #1994 fixed the shared warning bounds through [PR #1999](https://github.com/taskqorg/taskq/pull/1999); its board receipt records merge-result CI green at this integrated source. This task changes no implementation or overflow guard (§7 evidence-only scope).

The same command on the resumed source completed 50 actual WebView2 captures with **exit 0, no overflow diagnostics**, in 45.198 s including launcher/probe overhead:

```text
cargo run --locked -- tray --snapshot <menu-output> --route home,project,settings,appearance,hosts,cadence,notification-settings,agents,projects,delivery,services,recovery,about
```

The built-in instrument selects dark/light and nominal 16/20 px for these routes, except Project, which has only 16 px cases. No font14/18 menu, other DPI, real work-area anchoring, pointer, keyboard or last-control reachability pass follows from this result.

```text
cargo run --locked -- tray --snapshot <conversation-output> --route conversations
```

This completed ten native captures, **exit 0**, in 11.287 s including launcher/probe overhead. Its native snapshots do not call the menu geometry guard, and the fixture does not exercise real provider delivery. The sampled chat screenshot shows the selected reader near its composer; it does not prove older content is reachable.

`npm.cmd run build` followed by `node tests/<name>.test.cjs` passed for `relative-time`, `tray-answer`, `task-workspace`, `reports`, `reply-files`, `task-controls`, `tray-running` and `tray-settings`. [Component output](component-checks.txt) preserves the bounded claims. The initial PowerShell `npm` invocation was refused by its script execution policy; no policy changed. A component attempt before the explicit web-artifact rebuild failed at an assertion without preserving its full diagnostic; it is not a pass or an attributed product defect. The build and all eight checks then passed without source edits.

`cargo test --locked --bin taskq transcript::tests:: -- --nocapture`: **11 passed, 0 failed**, 12.02 s execution after 2m34s test compilation. The existing canonical helper supplied a fresh disposable TEMP/TMP/install and removed inherited attempt/session/credential bindings from the child environment. This includes 10 MiB cursor paging, 10,000 variable-height records in both directions, a 108 MiB source in 374 backward pages (4.1108906 s including writing), oversized-record refusal, partial/shrunk/replaced sources, native timestamp fixtures and credential-redaction checks. [Adapter output](transcript-checks.txt) records the result. These are source/adapter fixtures, not rendered performance, native provider use or long-history UI memory bounds. No test or assertion changed.

## Review set and limits

The [ten-screen index](screenshots.md) retains the seven earlier captures plus three new unmodified captures. Dark Home 16 and Settings 20 show the warning inside its bell target, with count 6 and separate uncertainty mark still visible. The light Conversations 18 capture retains the selected PM/draft surface. These images were inspected at native dimensions. There is no interaction recording or owner visual acceptance.

No production service, grant, saved view or real task answer was changed. Snapshot mode bypasses ordinary tray service startup; every launched snapshot selects the disposable installation, disables incident reporting and drops inherited task/session/board-token bindings in that child only. Full raw logs, captures, readback helper and generated fixture remain in the attempt's OS-temp scratch; repository evidence contains no private external paths.

The Computer Use inventory exposed no TaskQ or explicitly disposable Claude/Codex target window. This does not prove runtime unavailability. Ordinary tray startup can invoke service lifecycle operations and a host-wide tray lock; it was not used to bypass isolation. Under R3 only the service spawns owner/worker/reviewer sessions. A disposable service/board/provider environment has not been provisioned or exercised here. The available input tool injects events; it cannot establish the physical wheel/precision-touchpad requirement.

Remaining: physical/native interaction and accessibility, disposable provider/board correlation, real local/remote/ended targets, files and recovery, hostile-history rendered performance, all applicable native scales/monitor positions, exact Summary count/state matrix and recordings. These remain Not tested under R12. Owner note 6536639967 directs submission of those exact gaps rather than provisioning another environment; [G1–G3](../../../research/1933.md#proposed-tasks) propose their later qualification. No live owner answer substitutes for disposable-session proof.

## Synthetic-input continuation

After owner note 6536639967, the existing Computer Use `@oai/sky` helper successfully listed windows on the logged-in Windows host. No TaskQ target window was returned, both before and after the bounded fixture launch. No input was sent to another application or an unbound window.

The worker prepared only a disposable store in the existing attempt scratch, with an empty `projects` file and fixture `tray.json` selecting pinned 620×560, font20, Icon. Source inspection confirms an empty project list yields no project for ordinary startup's `lifecycle::open` to register. The exact already-built binary was launched through `cargo run --locked -- tray`, without `--open` or a global raise request, and with inherited task/session/board-token bindings removed from that child.

The child exited **1** before exposing a fixture window:

```text
Error: taskq tray already runs on this host
Next: Check the input and board/service status, then retry; use `taskq help` for syntax.
```

This is the existing native host-wide tray guard, not a sandbox denial or a proven UI defect. The worker did not move the lock, change TEMP to evade it, raise or stop the existing tray, register a fixture board, launch a provider, or send a synthetic gesture without a targetable window. Consequently actual SendInput/UI Automation navigation, keyboard, scrolling, draft, IME, archive and resize interactions remain **Not tested**. The host's absence of a targetable window does not establish that the product lacks those capabilities. [Bounded output](synthetic-input.txt) preserves the exact refusal and isolation boundary.

## Delivery checks

There are no product/test/config changes. The report and ten-screen packet are delivered as inconclusive evidence under the owner's note, with A7 complete and A1–A6 Not tested. `python scripts/check.py --base cbeb8db913aa0908fbe9490e7de8404e7a306231` passed with **child exit 0, 181.797 s / 900 s**, in fresh disposable TEMP/TMP. It selected the full suite: 746 unit checks passed (39 ignored), 19 integration checks passed (one ignored), offline selftest, Node components and artifact/release-script checks. [Canonical receipt](canonical-checks.txt) preserves the bounded outcomes.

The first invocation logged the same complete positive check outcomes in 185.688 s, then its outer wrapper failed decoding mixed-encoding output as UTF-8 before saving the child exit status. The unchanged gate was repeated solely to remove that uncertainty; the corrected wrapper saves the child exit first and reads the raw log with explicit decoding error handling. No test, assertion, timeout or product source changed. Both full logs remain in scratch. There were no retries inside either gate run. This records the failure before and pass after of the evidence wrapper, rather than inventing a product defect or claiming the first wrapper passed.

No live-board selftest or production rollout was performed. Green merge-result CI remains the PR publication gate under R12/§7, separate from native acceptance.
