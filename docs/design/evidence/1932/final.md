# Final native macOS evidence disposition

Attempt `6537243603`, observed 2026-10-31 01:37 UTC. [Report](../../../research/1932.md) and [criterion matrix](matrix.md) are the final entry points. **Overall native acceptance: incomplete.**

## Authority and frozen candidate

The [owner requeue](https://github.com/taskqorg/taskq/issues/1932#issuecomment-6537241947) directs no wait for #2003, no owner-run service or disposable owner sessions, use of the pinned build/own TMPDIR/synthetic input/fixtures, and submission with exact Not tested gaps. This permits reporting incomplete qualification; it supplies neither missing evidence nor permission to bypass R3/R9. R12 keeps A2–A6 unaccepted. No rule or guarding assertion changes (R13).

The existing `taskq-1932` worktree is continued from `7bd2ba55f852465eb5ec57e89b2e8e222e216e47`. Only Markdown evidence changes in this continuation. No second rebase, merge from main, implementation fix, native instrument edit or rebuild occurred (§7.4). [Integration](integration.md) remains the ancestor map for #1918 and #1926–#1931. Later moving-main changes are not claimed as tested.

Fresh SHA-256 readback:

| Artifact | SHA-256 |
|---|---|
| Isolated signed app executable | `44e1117c299fadae0d2cf59351a3787d8ee96971b465790e18c970fcfd719e2a` |
| `build/web/workspace.html` | `e4ebb95ecf37f063832e98136db5b89379d0991b98693a89d6482a16893f846a` |
| `build/web/tray.html` | `ad34e3d0276cdb69e73056852c61f46e56716ddf10ffbc57ea6860b22710ee66` |

All equal the [isolated manifest](isolated/manifest.json). TaskQ 4.6.50/dev, compiled source `67f729e4dc35d20f044efe8cb7a3b6a939c5e048`, separate bundle identity `org.taskq.qualification1932`. The same owned temporary install and TMPDIR were used; no production token, policy or service was added. Inputs are the existing 11 invented projects and two disposable conversation records. Old fixture observations are intentionally stale at this continuation, not healthy live board data.

Fresh read-only AppKit/WebKit output matches the retained [environment](isolated/environment.json): macOS 27.0.1/26A434, WebKit 22625.1.29.11.28, screen 1470×956 points, work area 1470×922, scale 2, `.AppleSystemUIFont`. Application appearance follows the dark system theme. Only disposable font/size/navigation preferences changed. No system display/theme or privacy setting changed.

## Attributable native checks

The actual signed product executable ran `tray --open` with its own `TASKQ_INSTALL`, `TMPDIR` and incident reports off. CUA selected that exact running app by its temporary bundle path. Element indices were rederived from fresh accessibility trees before dependent actions. Native screenshots were visually inspected during these checks; no additional delivered screen or recording was generated. The [ten-screen set](isolated/README.md#acceptance-and-review-set) and two sampled renderer clips remain unchanged.

| Requirement subset | Native action and observed result | Verdict and limit |
|---|---|---|
| S06, UX30-06, R12 | Home shows `Data from: unknown`, `Hosts ?`, Codex/Claude unknown, project counters unknown, stale/incomplete alpha/gamma/extras and explicit beta HTTP 502. | Pass for stale/partial display subset; fresh totals and conflicts are not exercised. |
| S07, UX30-07/08 | Click `Details for alpha #12`; one inline Ask panel reports no verified native session or published progress target. Parent title/state are not repeated inside it. | Pass for this Ask fixture; other states, keyboard disclosure and real target activation remain Not tested. |
| S08, Q-A1 | `app.scroll(panel, 'down', 1)` moves the main task list from the alpha title into the panel and beta/gamma rows. `app.scroll(panel, 'up', 1)` moves it back, with the host/counter strips and footer retained. | Pass for synthetic native wheel over this panel in both directions. No scrollTop assignment; physical wheel/trackpad and every title/control/boundary case remain Not tested. |
| S07, Q-A9 | Click `Close task details`; AX focus returns to the collapsed `Details for alpha #12` trigger. | Pass for Close/focus subset; full keyboard/VoiceOver/scrollbar-drag matrix remains Not tested. |
| S04, UX30-04 | Open Search; native paste `no-such-disposable-1932`; no task rows match. Search stays visible; `All tasks *`, unknown counters and `Tasks unknown` retain stale-data meaning. Escape returns focus to the search toggle without clearing the query. Clear restores rows and the collapsed search icon. | Pass for this no-match/Escape/clear subset; Held batch admission and all query/scope races remain Not tested. No saved user view is changed. |
| S05/S22, M25 | Open Settings → Appearance at 20 px, select 18 px, activate `Enlarge window`, inspect actual 1470×922-point window. Controls reflow across the wider view; heading/footer and lower growth controls remain visible. Restore 20 px. | Pass for this native enlargement/type subset; arbitrary resize, anchor tolerances and the whole theme/scale matrix remain Not tested. |
| S22, Q-A9 | Click footer `Resize window with arrow keys`, then Right twice; screenshot establishes no size change. | Not tested: keyboard resize was not established. No regression diagnosis from an automation attempt. Further identical attempts stopped. |
| S24, Q-A8/A11 | Native Cmd+Q; launching process exits 0. Fresh store assertions compare the draft and request with prior readback. | Pass for another clean client exit and unchanged-record subset, not pending-send/service recovery. |

The first CUA bundle-ID lookup failed because that temporary app was not resolvable by ID; selecting its existing bundle path succeeded. An initial assignment used an undeclared JavaScript binding and was corrected before interaction. These are tooling corrections, not application defects or successful tests.

## Fresh store readback

Python loaded the owned install's `tray-requests.json` and the one setup JSON, then compared them with [prior readback](isolated/readback.json). Assertions passed after clean exit:

```json
{
  "attempt": 6537243603,
  "application_exit": 0,
  "setup_record_count": 1,
  "request_id": "4eded416a3d6994c6f3e322b1ae9e59d",
  "request_state": "cancelled",
  "request_job": null,
  "request_session": null,
  "draft_id": "a1901456687af8ef47c7414e50bb90e4",
  "draft_equal": {"text": true, "files": true, "caret": true, "archived": true, "sent": true},
  "font_px": 20
}
```

No new request, PM message, board steer, real task answer or provider session was created. The previous client run's storage denial remains an unknown privacy/sandbox limitation, not a production grant diagnosis or owner ask. No service start or authority-refusal retry occurred.

## Exact remaining gaps and disposition

| Criterion | Final status | Missing evidence / reason |
|---|---|---|
| A1 | Pass for pinned candidate | Artifact/source/environment and integration identities remain explicit; later main is not included. |
| A2 | Not tested, partial passes | Synthetic native input is available; physical devices/IME, VoiceOver, successful scrollbar drag and complete route/font/theme/size/scale matrix were not exercised. No additional 1×/external-display cell was available in the pinned setup; notch/work-area combinations were not exercised. |
| A3 | Not tested | Worker service launch refuses R3/R9; owner says no owner-run service or disposable owner sessions will be provided. No two-turn Claude/Codex clarification/result chain exists; no live owner answer is used. |
| A4 | Not tested, partial passes | Fixture unavailable/partial state and native draft/file/archive/identity preservation are observed. Real local/remote/ended sessions, provider-backed file delivery, same-ID reconciliation and in-flight service recovery lack the permitted disposable context. |
| A5 | Not tested, measured subset | Retain the earlier 10,000-record/15,000-item, DOM ≤600 and input/receive/frame samples. No new benchmark ran. Real cold/warm 10/100 MiB I/O, hostile fixtures, JS-cache/process-memory plateau and full main-thread tasks remain unmeasured; WKWebView lacks the longtask observer. Proposed thresholds remain proposals. |
| A6 | Not tested, partial evidence | Existing native unknown/pause rasters and ten selected screens remain. Actual OS-slot A mixtures/ink/overflow/pause are not captured; other displays/scales are unavailable. Selecting Summary is not OS-slot acceptance. |
| A7 | Pass for evidence reporting | Report, full S/Q/UX30/preservation matrix, corrected findings, before/after links, measurements and one deferred-task proposal are linked. Native acceptance remains incomplete. |

[Report G1–G4 and proposed task](../../../research/1932.md#proposed-tasks) retain the missing observations. No new implementation defect is established. #2003 is historical preparation evidence, not a wait condition; no new follow-up was filed or started.

## Validation boundary

This continuation's canonical command is `python3 scripts/check.py --base 7bd2ba55f852465eb5ec57e89b2e8e222e216e47`; Markdown-only selection is `docs`. Its attributable outcome goes in the commit and task result. Local link, proposed-task JSON/limits, artifact identity and no-dispatch assertions also run. The prior broader [sandbox-blocked gate](isolated/checks.md) remains sandbox-blocked; a documentation pass does not replace its native/process gaps. No CI wait, merge, release or rollout is performed (R12, §7).
