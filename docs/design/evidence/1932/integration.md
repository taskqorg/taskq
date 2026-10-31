# Integrated task readback

Observed during worker attempt 6535498100. Every merge below is an ancestor of source `554552daf48d5b413918d54a8a6d0f88efba4020` (`git merge-base --is-ancestor`, exit 0). Board receipts report green merge-result CI. This establishes source integration, not native acceptance (R12).

| Task | State | PR | Integrated merge SHA |
|---|---|---|---|
| [#1918](https://github.com/taskqorg/taskq/issues/1918) | closed | [#1925](https://github.com/taskqorg/taskq/pull/1925) | `65e80d3835d40dddaa888dc1cf7ae8b85e93db24` |
| [#1926](https://github.com/taskqorg/taskq/issues/1926) | closed | [#1944](https://github.com/taskqorg/taskq/pull/1944) | `1ccf6e0526f44dcce2b9607562eef5beb65d9cf0` |
| [#1927](https://github.com/taskqorg/taskq/issues/1927) | closed | [#1948](https://github.com/taskqorg/taskq/pull/1948) | `b0c5269e498824d208ddd04e4da86b3c0d45b45f` |
| [#1928](https://github.com/taskqorg/taskq/issues/1928) | closed | [#1955](https://github.com/taskqorg/taskq/pull/1955) | `82fb5dc76b4a7db4dec83dea92d7b00bcb2e5ac9` |
| [#1929](https://github.com/taskqorg/taskq/issues/1929) | closed | [#1961](https://github.com/taskqorg/taskq/pull/1961) | `efce57d9295abd72184778d2ab32ab4e46a683f0` |
| [#1930](https://github.com/taskqorg/taskq/issues/1930) | closed | [#1986](https://github.com/taskqorg/taskq/pull/1986) | `554552daf48d5b413918d54a8a6d0f88efba4020` |
| [#1931](https://github.com/taskqorg/taskq/issues/1931) | closed | [#1947](https://github.com/taskqorg/taskq/pull/1947) | `b3b5c639e94820f3d37bb27985f7bfc54166805a` |

## Trusted close receipts

### #1918

```text
taskq: close
Changed: [PR #1925](https://github.com/taskqorg/taskq/pull/1925): tray surfaces read one snapshot, answers confirm by op-id readback, conversation keeps every Q&A, worker steps fold; rule gate counts approvals after an attempt requeue
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok (A1-A6 verified at c076a0aa; delta to a4e414d1 is scripts/check.py only); merge-result CI pending on a4e414d1
Checks: PR #1925 CI at a4e414d1 -- plan (rule gate: changed M8), rust ubuntu, tests, shots pass; python3 scripts/check.test.py at a4e414d1 -- PASS
Checks: Implementation quality: none binding; advisory: gate still reads gone/usage-limit as unknown (conservative, fails closed)
Open: none

delivery auto-merge r0: merged #1925 as 65e80d3835d40dddaa888dc1cf7ae8b85e93db24; merge-result CI green
```

### #1926

```text
taskq: close
Changed: Menu and all 12 Settings routes share centered status slots, icon headings, 16px/4px rhythm and a neutral project rail; the counter row sticks opaque ([#1944](https://github.com/taskqorg/taskq/pull/1944)).
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok, A7 ok (browser projection; native gap below); sticky note ok top/middle/bottom; merge-result CI pending (PR CI green on 922362f5)
Checks: reviewer rerun at 922362f5: ui-polish (416 cells), tray-background, menu-project-groups, settings-content, tray-view-state, conversations-sidebar, task-dashboard, display-model -- PASS
Checks: Implementation quality: no binding gaps; taskq.md 294 lines/61,438 bytes within R8; Product/M2/M3/M1.2 edits match owner answer 1
Open: Native macOS/Windows rendering stays with #1932/#1933 (R12); no install/deploy.

delivery auto-merge r0: merged #1944 as 1ccf6e0526f44dcce2b9607562eef5beb65d9cf0; merge-result CI green
```

### #1927

```text
taskq: close
Changed: Collapsed strip shows per-host totals then overall Codex/Claude; expansion has shared Host/Codex/Claude/Total columns with 0/?/N+? ([#1948](https://github.com/taskqorg/taskq/pull/1948)).
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 gap native WKWebView/WebView2 comparison (routed to #1932/#1933 per ui-polish.md); merge-result CI pending (PR CI green on 50c076da)
Checks: reviewer rerun at 50c076da: tray-running, menu-host-strip (24 cells), settings-content, tray-panels -- PASS; collapsed/expanded screenshots inspected
Checks: Implementation quality: no binding gaps; advisory: Total column header is an icon only (name via title/aria)
Open: Native macOS/Windows rendering and disclosure gesture stay with #1932/#1933 (R12); no install/deploy.

delivery auto-merge r0: merged #1948 as b0c5269e498824d208ddd04e4da86b3c0d45b45f; merge-result CI green
```

### #1928

```text
taskq: close
Changed: PR #1955: all eight open states expand inline in the list scroll with direct issue, agent, native and progress routes; queued review reads merge queue · CI.
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok, A7 ok, A8 ok; owner merge-queue note ok; merge-result CI pending on 23c9d24e
Checks: node tests/task-navigation.browser.cjs, closed-pins, project-blocks, task-controls, task-workspace, tray-task-list, task-dashboard, conversations-selection -- PASS (mac reviewer rerun after npm run build)
Checks: Implementation quality: none binding; advisory: queued CI progress shows unknown until a publication reason exists
Open: native WebKit/WebView2 and OS link checks stay in #1932/#1933.

delivery auto-merge r0: merged #1955 as 82fb5dc76b4a7db4dec83dea92d7b00bcb2e5ac9; merge-result CI green
```

### #1929

```text
taskq: close
Changed: PM brief-to-result flow in one chat; Windows transcript generations now use native file IDs, so replacement is detected (A4). [PR](https://github.com/taskqorg/taskq/pull/1961)
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok, A7 ok, A8 ok; merge-result CI pending on pending; PR CI green at 723d67e5
Checks: diff f61f5130..723d67e5 -- only transcript.rs generation (FILE_ID_INFO via existing windows crate feature), its test, testing.md note; code otherwise unchanged
Checks: prior merge_group run 41108123051 -- only failure Windows transcript::tests replacement test; Windows test now forces equal creation time
Checks: cargo test --locked --bin taskq transcript::tests at 723d67e5 on macOS -- 10 PASS
Checks: Implementation quality: none binding
Open: Windows execution proven only by merge-group CI. Native Mac/Windows checks #1932/#1933 unqualified (R12).

delivery auto-merge r0: merged #1961 as efce57d9295abd72184778d2ab32ab4e46a683f0; merge-result CI green
```

### #1930

```text
taskq: close
Changed: Conversations show relative ages with exact local date, seconds and zone on hover/focus, minute refresh in place: PR #1986 (UX30-17..19, D-A19).
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok, A7 ok (source/adapter; live remote/native not tested, disclosed); merge-result CI pending
Checks: Reviewer rerun at 67f266f9: conversation-times.browser PASS Chromium+WebKit 24 cells; sidebar/dialogue/worker-panel browser, relative-time, task-workspace PASS
Checks: cargo test 1930 provenance, sidebar_activity, fake_relay, status_json golden -- PASS; PR CI rust ubuntu green
Checks: Implementation quality: no binding gaps; advisory: 14ch time box wraps sidebar meta to a second line at narrow width
Open: history/brief/result times stay Time unknown until the board adapter records them; native VoiceOver/WebView2 qualification in #1932/#1933 (R12).

delivery auto-merge r0: merged #1986 as 554552daf48d5b413918d54a8a6d0f88efba4020; merge-result CI green
```

### #1931

```text
taskq: close
Changed: Summary A six-bin stacked New/Completed/Ask/Failed icon and History on one Rust path; New history resumes from status.json with five event reads per pass (PR #1947).
Checks: Task fit: A1 ok, A2 ok, A3 ok, A4 ok, A5 ok, A6 ok, A7 ok (native shell/Dock, Windows Not tested, as allowed); merge-result CI pending on merge_group
Checks: Round 3 fix: cargo test --locked --bin taskq incoming -- 6 PASS at efc2508a; cold <=5 sources/pass, restart reads no settled history, failed source keeps cursor
Checks: PR #1947 CI at efc2508a -- rust, tests, plan, shots pass; windows-gnu skipped
Checks: Implementation quality: none binding; R13 amendments M1, M6, I3, R-UI4 per PM note 6525411430
Open: Live cross-host restart and OS shell/Dock/Windows native placement Not tested.

delivery auto-merge r0: merged #1947 as b3b5c639e94820f3d37bb27985f7bfc54166805a; merge-result CI green
```

## Resumed candidate

Attempt `6536345627`: all nine recorded source/merge SHAs are ancestors of `67f729e4dc35d20f044efe8cb7a3b6a939c5e048`; application sources, tests, Cargo files and taskq.md equal main `cbeb8db913aa0908fbe9490e7de8404e7a306231`. No already-published commit was rebased.

| Correction | Fresh board readback | Integrated merge SHA |
|---|---|---|
| #1991 / PR #1998 | Closed; Send visible in the byte-identical original image; no layout fix; refusal Next restored | `34f21b778330bf961dc63c542522b5b3cd2bcbd2` |
| #1994 / PR #1999 | Closed; warning bottom/right 0, guard unchanged; merge-result CI green | `cbeb8db913aa0908fbe9490e7de8404e7a306231` |
| #1992 | Closed as consolidated into #1994; no separate implementation | — |

The fresh #1991/#1994 close receipts explicitly end with merge-result CI green. This is readback of implementation publication, not this qualification task's CI or native acceptance.
