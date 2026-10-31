# Existing-host qualification

Reports and finished tasks render in Conversations with the grouped sidebar and shared decision cards. The rebased candidate retains experiment verdicts, CLI Decide revisit dates and the [approved dated Later flow](https://github.com/taskqorg/taskq/issues/1977#issuecomment-6547159713). Native rendering, installed activation and physical input are separate claims (R12).

The [owner continuation](https://github.com/taskqorg/taskq/issues/1977#issuecomment-6546915208) drops the disposable Mac prerequisite and requests explicit Not tested cells on the existing hosts. This evidence does not replace the owner release batch under §7.8.

## Candidate and environment

Source: `51b6c40634c1589f4e637c4c93ec6a9a1c07f300`, rebased on `49ebbe4addf65807a99197c919115c8f512c5b70`. [Capture metadata](capture-rebase.json) records executable, loader and image hashes. The executable and x64 WebView2 loader were relocated together into an isolated OS temporary store; both snapshot routes ran directly there. No service was registered, replaced or restarted. Invented IPC receipts prevent real decisions and task creation.

Windows WebView2; configured 16 CSS px; display DPI 96 (100%); owner TextScaleFactor 140%; primary display 2560 by 1440 and work area `(0, 0, 2560, 1392)`. The 1280 by 760 logical window produces 1281 by 761 physical-pixel captures including its native edge. Settings were read, not changed. The approved reference uses 100% text size, so the comparison establishes structure and wrapping on this host, not pixel equality. Fixture epoch timestamps do not qualify recency or collection freshness; the unknown verdict is also invented fixture data.

The installed Windows baseline remains v4.6.56. Its executable SHA-256 was freshly read back as `32193943edc9ee5c9135b1c2863df7462735293268d1a8e7ed4ef3056e9e2479`, matching the retained baseline captures. Earlier candidate frames and [their metadata](capture.json) remain historical evidence, outside the current review set.

## Six-screen native review set

| Frame | Observation |
|---|---|
| [Installed report, dark](installed-report-dark.png) | Retained v4.6.56 baseline: separate reader, no Conversations sidebar |
| [Rebased report, dark](candidate-rebase-report-dark.png) | Grouped sidebar and filters remain beside report content and its numbered decision card |
| [Rebased report, light](candidate-rebase-report-light.png) | Same reading and decision structure in the other theme |
| [Installed finished task, dark](installed-finished-dark.png) | Retained baseline before the merged reader |
| [Rebased finished task, dark](candidate-rebase-finished-dark.png) | Changed/Checks/Open and review link remain in the selected Conversations pane |
| [Rebased finished task, light](candidate-rebase-finished-light.png) | Same finished-task structure in the other theme |

Compare with approved mockups [2](../02-report.png) and [3](../03-finished.png). App chrome, content and owner text scale differ. Sidebar placement, filters, report-first reading and primary choice order follow the approved model. Controls below the fold, dated Later entry, IME and native accessibility were not exercised through native input. The [seventh browser reference](../07-later.png) illustrates the approved date-entry addition; browser instruments verify its delivery, validation and retry behavior.

Native commands, substituting only temporary capture destinations:

```text
<relocated candidate> tray --snapshot <report captures> --route reports-conversations
<relocated candidate> tray --snapshot <finished captures> --route results-conversations
```

## Acceptance and remaining cells

| Acceptance / cell | Evidence | Status |
|---|---|---|
| A1-A4 | Approved Product design, five-job table, six original mockups and dated Later continuation | Retained |
| A5 implementation | Bundled Conversations reader, redirects, exact-SHA navigation and current native frames | Pass for candidate behavior |
| A5 live Windows activation | Existing installation remains the baseline | Not tested; owner release batch under §7.8 |
| A5 Mac dev install | SSH refused before authentication; no remote command ran | Not tested; existing host inaccessible |
| A6 Windows reading | Both kinds/themes at measured display/text settings | Partial; structural comparison, not pixel equality |
| A6 Mac WKWebView | Fresh `ssh -o BatchMode=yes -o ConnectTimeout=10 macbook-m2.local` returned `banner exchange: Connection to UNKNOWN port -1: Connection refused` | Not tested; no Mac capture or version readback |
| A6 native input | Physical input, date selection, lower-card scroll, IME and accessibility | Not tested; owner session or physical input required |
| A6 decision/grouping protection | Canonical 16-cell report browser matrix and final bundled sidebar instrument | Pass at candidate source |

## Verification

`python3 scripts/check.py --base 49ebbe4addf65807a99197c919115c8f512c5b70` passed on the exact source above in **433.828 s / 900 s**, no retries: 793 Rust tests and 19 integration tests passed; 40 Rust and one integration case remain ignored. Isolated offline selftest, Node/component/artifact/release checks and all 16 report browser cells passed. Offline board/tool probes remain unknown by design, not live acceptance.

The local full gate reported contract lint unknown because its pinned checkout was absent. The exact CI-pinned SpecQ revision `39e6024037fb69e68a84180c04fb7ac53eddb9f9` was then fetched and its identity verified. `python3 scripts/check.py --lint` separately passed: 47,273 UTF-8 bytes, 234 lines, longest visible line 394 characters, zero errors. Lint thresholds and existing size guards are unchanged. The rule gate reports only Product amended under R13.

`node tests/conversations-sidebar.browser.cjs` passed on the final bundle: grouping/recency, pin/collapse receipts, search/reopen, Archive/Restore and retained drafts/focus. `node docs/design/exploration/reports-conversations.check.cjs` passed the original 72 geometry cells and the dated Later interaction. These instruments use invented receipts; they do not qualify real board writes or native input.

Failure before / pass after:

- The merged adapter lost `until`; `tests/reports.test.cjs` failed with `undefined` instead of the selected date. It now passes while retaining future-date validation and Decide revisit behavior.
- Failed read acknowledgement caused another read on redraw (two requests instead of one). The regression now passes: one automatic attempt per displayed source, one new attempt on explicit reopen.
- Unknown Accept followed by another host's Make tasks left correction capability false after authoritative collection. Its regression now passes: same-SHA terminal readback settles the loser without a new decision write. Unknown correction/successor creation remains fenced.

The shared-card tests retain immutable SHA/request identity, source-numbered choices, held-only creation, first-winner Make reservations, closed originals and decided history. Read-only review verified the two receipt fixes. Green merge-result `merge_group` CI remains the publication gate (§7, R12); neither local success nor these frames prove publication or installed activation.
