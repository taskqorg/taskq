# Settings content · #1661

Candidate for [#1661](https://github.com/taskqorg/taskq/issues/1661): hosts, agent sign-in, services and compact route widgets. Source: #1657 review-handoff R16–R20 and R28–R35.

Source checks are Rust unit tests and Node tests over the real controller (`tests/tray-harness.cjs`). Browser checks run the bundled `build/web/tray.html` in the Chromium headless shell (`tests/browser.cjs`) with invented data and a stubbed IPC. Neither reads a real host, a real `status.json` or the OS notification permission. Those items are native and are reported apart.

## Ledger

| R | Source/browser | Native | Evidence |
|---|---|---|---|
| R16 named host lines | Pass | Not tested | `tray-running.test.cjs`, `settings-content.browser.cjs`: one `details.host-line` per named host, `Claude N · Codex M` apart; a stale host reads `runs unknown · 5m old`, never 0; the Settings widget reads `3 hosts · Runs unknown`, never one host's name for a global count |
| R17 host expansion | Pass | Not tested | This host: `CPU 42% busy · memory 62% available · sampled 2m ago` (`display.rs` `sample_age_text`), occupied slots, reservations and board tasks on separate lines; no sample reads `CPU and memory: no sample; sampled only while the load gate is on`; remote hosts read `not collected` |
| R18 versions, account, expiry | Pass | Not tested | `runtime.rs` `agent_view` (AS5 view); `tray-settings.test.cjs`: `Tray 4.1.0 · service 4.1.0 (2)` apart from `Runtime client 2.1`; the account (`@` address) beside the state, `ChatGPT` and subscription only as `Method`; `Expires …` when supplied, else `Expiry unknown`; check age |
| R19 distinct sign-in states | Pass | Not tested | Signed in, Signed in at <time> (probe older than 360 s), Checking, Check failed, Unknown, Expired, Rejected and Sign in required each render apart; the sign-in route names the native command and `A check never signs in.`; a remote host reads `sign-in unknown until this host reports`. Checking: before a due probe the service writes `agents.checking` into the last `status.json` (`main.rs` `a_due_probe_marks_the_last_status_checking`); the tray reads it only when newer than the freshest probe and at most 60 s old (`tray.rs` `agents_come_from_the_freshest_probe_or_are_unknown`); rows read `Checking` with `Last result: <state>` in the detail, the widget `checking sign-in` (`tray-settings.test.cjs`). A real probe in progress on a host is native |
| R20 one Appearance route | Pass | Not tested | Text size buttons render at their own size; tray icon previews and the four window growth previews sit on one route; with reduced motion the previews are still dashed outlines with ↓/↑ and the page says so (dark capture runs with `reducedMotion: reduce`) |
| R28 compact widgets | Pass | Not tested | Twelve widgets with live summaries at 320 px, no clipped text; every widget route renders (`tray-settings.test.cjs`); the `window` and `tray-icon` routes fold into Appearance, and the native icon capture (`tests/fixtures/icon-history/capture.swift`) opens `appearance`; Hosts and resources is a route |
| R29 Access without a placeholder | Pass | Not tested | The workspace Agents placeholder is gone; Agents and access shows requested, saved grant, applied and enforcement per runtime, a refused grant with its reason, `Access unknown for <project>: no access in status.json`, and one editing reason naming `taskq service permissions` |
| R30 services once per host | Pass | Not tested | `Host service: running · PID 42` appears once; project rows show their status age only; mixed summaries wrap whole words (`summary.line`) instead of a one-letter column |
| R31 Projects, Delivery, About | Pass | Not tested | Request prose sits in a closed `Request` disclosure; Delivery groups presets (`auto-merge (3)`) with the revision in details; About groups equal service versions (`Service 4.1.0 · 2 projects`) and keeps installed, available and unconfirmed update checks apart |
| R32 Work mode | Pass | Not tested | The widget and page show admission first (`3 admitting`), then `capacity unavailable` or the resource view, then the access facts themselves: per runtime requested, saved grant, applied and enforcement, or `refused: <reason>`, and `Access unknown for <project>: no access in status.json` (`tray-settings.test.cjs`, the same renderer as Agents and access); projects without a reported mode read `admission unknown`, never `no projects` |
| R33 Notifications | Pass for source | Not tested | OS permission is its own row; per-event choices sit under `Per event on this host`. The real OS grant and test notification are native |
| R34 Activity legend | Pass | Not tested | One period line `start – end`; `Counts are events, not current tasks.` (`tray-history.browser.cjs` passes) |
| R35 Recovery per target | Pass | Not tested | Each project shows `Not read`, `Reading…`, `Read at <time>` or `Outcome unknown` with its next action (`tray-settings.test.cjs`) |

Native items not run here: real host data on Mac, Windows and FRA, real sign-in probes, the OS notification permission, and the tray windows. They stay Not tested until a tray run on each host.

## Screens

Ten browser captures at 320 px CSS width, written by `SETTINGS_EVIDENCE=docs/design/evidence/1661 node tests/settings-content.browser.cjs`. Dark captures run with reduced motion.

| File | Shows | SHA-256 |
|---|---|---|
| [settings-320-light.png](settings-320-light.png) | Settings index: twelve compact widgets | `e3537a1c733b805f44d98234aebb6470b86c2a9a1150330c35e80cd883cf970f` |
| [settings-320-dark.png](settings-320-dark.png) | Same, dark | `fda4093064477033998b78d103f8d6575f54d6746d67f79687af5f00f991afd0` |
| [hosts-320-light.png](hosts-320-light.png) | Host lines, this host expanded with CPU and memory | `710a5a5344cd1bce1e67b26a2f4b4ac5d99f3103a8a8b67a187dfbaf509dbfb2` |
| [hosts-320-dark.png](hosts-320-dark.png) | Same, dark | `1baa5ad32b77260b052b794da2d94766bc12e1e12480822fd0149376dc5147fb` |
| [agents-320-light.png](agents-320-light.png) | Sign-in per runtime, Expired expanded, access expanded | `fc05af30682baadbbfe380d0a5f781fa8e85f1e800652634aa89a9e8f1c87ff7` |
| [agents-320-dark.png](agents-320-dark.png) | Same, dark | `fe21c78f086eb4a27ac774b450c8bfc68bb5d4a317e479c2c6520c7c419f99c8` |
| [appearance-320-light.png](appearance-320-light.png) | Text size, tray icon and window growth previews | `9b0ec304cee32f83f43afb60421f79bbb58bf0504da608813c767298fde7d279` |
| [appearance-320-dark.png](appearance-320-dark.png) | Same, dark, reduced motion: still outlines with arrows | `59d9fde58c8ad77160f9e5a437cbe28e3f9fcd2d077756e0d936be0025a7f124` |
| [services-320-light.png](services-320-light.png) | One host service line, project rows with status age | `0c4d42fe84339548034ec33611012c61ca5ddfcc5256596bed9efae624f8b051` |
| [services-320-dark.png](services-320-dark.png) | Same, dark | `eb7000e77403cead385ded96f7f1526d5d93a5f8a790ede4d224bc185771cbf4` |
