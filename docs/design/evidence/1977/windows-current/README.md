# Existing-host qualification

Reports and finished tasks render in the candidate's Conversations pane with the grouped sidebar and shared decision cards. This is native Windows renderer evidence, not live activation or complete A5/A6 acceptance (R12). The [owner continuation](https://github.com/taskqorg/taskq/issues/1977#issuecomment-6546915208) drops the disposable Mac prerequisite and requests explicit Not tested cells on the existing hosts.

## Candidate and environment

Source: `aa2cea419fd7a7bcb0e962889983e8fcedd489ae`. The only source change after `a2570a31` makes the two result snapshot routes use the approved 1280 by 760 logical window; the normal installed window keeps its existing size. [Capture metadata](capture.json) records executable, loader and image hashes.

Windows WebView2; configured 16 CSS px; Windows display DPI 96 (100%); owner TextScaleFactor 140%. Primary display 2560 by 1440, work area `(0, 0, 2560, 1392)`. Captures are 1281 by 761 physical pixels, including the native capture edge. Text scaling was read, not changed. The approved browser reference uses 100% text size, so these frames establish structure and wrapping on this host, not pixel equality to that reference.

The candidate executable and its x64 `WebView2Loader.dll` were relocated together into an isolated OS temporary directory. Both routes started directly there without Cargo's loader PATH. The store was isolated; no service was registered, replaced or restarted. The actual installed Windows baseline remains v4.6.56. Invented IPC receipts prevent real decisions and task creation. The existing fixture uses epoch timestamps, so the displayed age and freshness are not recency qualification.

## Six-screen review set

| Frame | Observation |
|---|---|
| [Installed report, dark](installed-report-dark.png) | v4.6.56 has the separate reader with Make tasks/Later/Drop and no visible Conversations sidebar |
| [Candidate report, dark](candidate-report-dark.png) | Project grouping and All/Needs reply/Needs decision/Finished stay beside Question, Recommendation and the numbered Accept/Later/Drop card |
| [Candidate report, light](candidate-report-light.png) | Same reading and decision structure in the other theme |
| [Installed finished task, dark](installed-finished-dark.png) | Baseline before the merged reader |
| [Candidate finished task, dark](candidate-finished-dark.png) | Changed/Checks/Open, review link and shared question remain inside the selected Conversations pane |
| [Candidate finished task, light](candidate-finished-light.png) | Same finished-task structure in the other theme |

Compare the candidate report and finished-task frames with approved mockups [2](../02-report.png) and [3](../03-finished.png). Titles, fixture content, existing app chrome and the owner's text scale differ. The shared sidebar destination, filter meanings, report-first reading and decision question order match the approved model. The long reader scrolls; controls below the fold are covered by the browser instrument, not by native input in these stills. Earlier narrow captures under `../windows/` are historical instruments, outside this six-screen review set.

Commands, with temporary store and evidence destinations substituted:

```text
<installed executable> tray --snapshot <baseline> --route reports-conversations
<installed executable> tray --snapshot <baseline> --route results-conversations
<relocated candidate> tray --snapshot <candidate> --route reports-conversations
<relocated candidate> tray --snapshot <candidate> --route results-conversations
```

## Acceptance and remaining cells

| Acceptance / cell | Evidence | Status |
|---|---|---|
| A1-A4 | Approved Product design, five-job table and six mockups | Retained |
| A5 implementation | Bundled report/finished reader, legacy redirects and exact-SHA navigation; native candidate frames above | Pass for candidate behavior |
| A5 live Windows activation | Existing installed v4.6.56 is still the baseline; no candidate activation occurred | Not tested; publication and owner release batch remain under §7.8 |
| A5 Mac dev install | SSH failed before authentication; no Mac command or installation readback ran | Not tested; existing host is inaccessible from this attempt |
| A6 Windows native reading | Both result kinds and themes at the measured display/text settings | Partial; structural comparison only, no pixel-equality claim |
| A6 Mac WKWebView comparison | `banner exchange: Connection to UNKNOWN port -1: Connection refused`; hostname and direct IPv4 attempts failed | Not tested; no Mac screenshot or installed version can be attributed |
| A6 native input / owner-session cells | Physical input, IME, native accessibility and lower-card scrolling were not exercised | Not tested; require owner session or physical input |
| A6 decision and grouping protection | Canonical report browser matrix plus existing sidebar browser instrument | Pass at candidate source |

No disposable Mac context, replacement task, permission change or real board reaction was created. A5 live activation and the missing A6 cells remain unknown; these frames do not silently complete those acceptance items.

## Verification

`python3 scripts/check.py --base 554552daf48d5b413918d54a8a6d0f88efba4020` passed on the source above in **299.250 s / 900 s**, no retries: 744 Rust tests passed, 39 ignored; 19 integration tests passed, one ignored; isolated offline selftest, Node/component/artifact/release checks and all 16 report browser cells passed. Ignored/native cases remain unknown. The final commit adds evidence only and does not change tested source, tests, dependencies or contract.

`node tests/conversations-sidebar.browser.cjs` passed on the same bundled candidate: mixed grouping/recency, pin/collapse receipts, search/reopen, Archive/Restore and retained drafts/focus. Its receipts are invented; no execution effects occurred.

Failure before: installed v4.6.56 hides the sidebar while reading a report; the previous candidate snapshot is only 800 by 600, too narrow for the approved wide reference. After: the native candidate shows the sidebar and shared card at the approved wide size. Earlier report-Accept rejection, stale-receipt and missing-composer-manifest defects and retained guards are recorded in the [design review](../../../reports-conversations.md#checkpoint-and-remaining-evidence). Green merge-result CI remains the publication gate (§7, R12); this local gate does not establish it.
