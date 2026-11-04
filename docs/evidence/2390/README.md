# Live UI bundle adoption proof

Task #2390 finishes #2365 live A2. R8 and R12 keep source, native and temporal proof separate; unknown stays unknown. No rule changed.

**Status: A1, A2, A3, A4 and A5 have evidence.** The first run left A4 partial on WebView2, because the composer attachment was not exercised and the visible reading row moved. The retest under PM decision `pm-2390-retest-a4-not-waive` closes that gap. All times are UTC on 2026-11-04.

## A4 retest on WebView2

The A1 to A5 sections after this one describe the first run, and they are unchanged. The retest baseline is in `retest-baseline.json`, timing is in `retest-timing.json`, samples are in `retest-store-samples.jsonl` and `retest-watch.log`, and the picker findings are in `retest-picker.md`.

**Publication.** PM moved `ui-live` to `3bc1c84f81d72e172940a568b40efff1946c7f73` (PR #2399, merged at 21:54:35, merge-group CI 41920350319 as PM reports). Run 41925130100 published bundle `1793829225-3bc1c84f81d7`, bridge 1. The id equals the commit time and SHA prefix.

**This check was automatic.** The tray's own 15-minute poll fired, with the previous empty check at 22:21:19. PM later allowed one manual About check, and none was pressed. No debug flag or local publication override was used.

**Source boundary.** `b9cbba8f..3bc1c84f` changes only docs/evidence, docs/testing.md and scripts. The new bundle therefore proves reload and retention, not a visible change. PR #2371 stays the visible change for A2, as the first run shows.

| Step | Time | Seconds after pointer |
|---|---|---|
| Pointer asset created | 22:25:52 | 0 |
| Check starts (first scratch folder) | 22:36:19.806 | 627.806 |
| Bundle staged in the store | 22:36:23.246 | 631.246 |
| `ui/state.json` current is the new bundle | 22:36:42.630 | 650.630 |
| Conversations page gone (first sample) | 22:37:16.059 | 684.059 |
| Conversations new page (first sample) | 22:37:18.122 | 686.122 |

Bottlenecks, largest first:

1. **Poll wait: 627.806 s, 96% of the 650.630 s to commit.**
2. **Conversations reload: 52.813 s after staging and 33.429 s after the commit.** In the first run this took at most 2.77 s. The cause was not established, because the page keeps no record of a declined offer. This proof sent no input to this window after the 22:21 baseline, and owner input was not sampled. The source allows a retry every 15 s, and the page declines while it has input within 10 s or records in flight. `tray-requests.json` was written 67 ms before the page went, which fits the normal flush path. The commit came first, so another page satisfied the clean-start trial. Which page did was not read.
3. Publication: 31 s from run creation to the pointer asset. PM held the push until this baseline was ready, so merge to push is not a system delay here.
4. Check to commit: 22.824 s, with 3.440 s of download and verification.

Retention:

| Item | Before publication | After the reload |
|---|---|---|
| Service, tray | 41960 (19:50:55.158), 52132 (19:51:14.151) | Same PIDs and start times at 22:40:00 and 22:42:37 |
| Binary | v4.6.78, `58a66672…` | Same at 22:41:42, and About shows `Tray installed 4.6.78`, `Service 4.6.78` |
| Selected conversation | taskq#2390 | Same in the store and on screen |
| Draft | `taskq#2390 controlled unsent draft - do not send` | Identical in the window at 22:37:18 and in the store at 22:41:42 |
| Attachment chip | `taskq-2390-controlled-attachment.txt · 63 bytes`, with Remove | Same chip and Remove button at 22:37:18 and 22:37:24 |
| Attachment bytes | Record and stored file SHA-256 `7558a886…cf114` | Record and stored file hash identical at 22:41:42 |
| Reading anchor, stored | `{at:1544207,i:0,y:14,attempt:6642610592}` | Identical at 22:41:42 |
| Reading row, semantic | `6 steps · 6 commands` at the top of the viewport, row y 413 | Same row, row y 413, composer y 476 |
| Page | `42.788308.4.11.1.37` | `42.657432.4.19.1.37`, with one reload and no second one to 22:42:24 |

The reading position held on the semantic row and in the stored anchor. Pixel values are informational: history y was -1405 at the baseline, -2876 after the resume at 22:26:29, and -1405 again after the reload, while the row stayed at y 413. Caret and focus are not claimed.

Workers:

- Codex PIDs 43820 (21:31:41.900), 40548 (22:25:07.918) and 55188 (22:29:03.127) are service descendants. All three kept their PIDs and start times through adoption, at 22:40:00 and 22:42:37.
- This Claude worker, 35316 (21:57:35.957), stayed alive through the ask and through adoption. Its ancestry runs through WmiPrvSE, so it is not shown as a service descendant.
- Baseline PIDs 53964, 54300 and 31032 ended before publication, so they say nothing about adoption. The pointer came at 22:25:52. In `retest-watch.log`, 53964 and 54300 last appear at 22:21:21 and 22:23:53, and 31032 never appears, because it ended before the next logged sample.
- Task mapping was not read from the service. Menu ages at 22:40:55 (#2366 1 h, #2377 16 min, #2393 12 min) only fit the Codex start times.
- Linux and Mac production workers were not measured. Mac real adoption is PM's fixture from the first run, and PM asked for no repeat.

No replay: `op` and `receipt` stay null, nothing was sent, and the only board comment holding the draft phrase is still PM's earlier Mac note, 6641071072.

Native images, each from the real window through `PrintWindow` and each inspected:

- `native-conversations-retest-before.png`: old page before publication, with the draft, chip and reading row.
- `native-conversations-retest-adopted.png`: new page at 22:37:35, with the same draft, chip and reading row. This proof had sent no input to this window since the baseline.
- `native-menu-retest-adopted-sanitized.png`: menu at 22:40:55, footer TaskQ v4.6.78. The host row is masked, and rows below taskq are removed.
- `native-about-retest-adopted.png`: `Interface 1793829225-3bc1c84f81d7 · adopted`, captured at 22:41:56.

Not proved or still open:

- The attachment came through real paste, which is a supported route. The picker route was not proved. Closing a Windows common file dialog hangs on this host, also in a separate process without TaskQ. See `retest-picker.md`.
- Reload with the native picker open was seen once in the first run and was not retested. A file being chosen during that reload might be lost, so it stays a possible follow-up.
- The native #2363 Rust changes are not in binary 4.6.78, and this proof does not show them. The native binary release follows this proof.

Actions on the owner's real tray in this retest:

- One synthetic tray click and UI Automation on `Open taskq#2390`, which moved selection from `serverq#47` and showed Conversations.
- Real clicks on Attach at 22:08:42 and about 22:12:10, plus a file name by `WM_SETTEXT` and real keystrokes. I stopped my picker helpers 10656 and 10952, and my own probe dialogs and their children.
- A real Ctrl+V with the clipboard set and cleared. The clipboard held no formats before this.
- A Chrome probe with an isolated temp profile, launched twice. No click landed, and my processes were confirmed gone.
- Foreground switching, and after adoption two more synthetic tray clicks and UI Automation on Settings, About, Remove, the draft, List and Return to view.

Cleanup at 22:42 to 22:45: the chip was removed through its button, the draft is empty with no files in its record, the 63-byte controlled copy stays in the install's reply-file store because Remove only detaches it, selection is back on `serverq#47`, Settings and Conversations are closed, and the samplers are stopped. Tray and service were never restarted. Channels and logins were not changed. Send was never pressed. `ui/state.json` stays on the adopted bundle, as normal publication intends.

Reproduce: use the Reproduce section below with `retest-sampler.cjs` and `retest-watch.ps1`, run 41925130100 and commit `3bc1c84f81d72e172940a568b40efff1946c7f73`.

## A1 Baseline

`baseline.json` holds the observation from 19:59:19.

- Windows: released v4.6.78, binary SHA-256 `58a666729494071c26e9b2be9b3a58657b5d59e13716e078cec2b55debaaf60a`, equal to the release asset. Embedded bundle `1793820775-97cf9558ce92`, bridge 1, source `97cf9558ce92e067796fc309612ea471093850cd`. No `ui/state.json` existed.
- Linux: the board heartbeat reports v4.6.78. Its binary, embedded bundle and native page were not read here.
- Mac production: the board heartbeat reports dev v4.6.79-261104-10. Dev hosts do not follow the UI channel, so this host is excluded. PM measured a private released-binary fixture instead.

The local 19489 ms rehearsal and an equal embedded bundle are not used as adoption evidence.

## A2 Change used

This proof reuses existing compatible work and adds no UI change: PR #2371 for task #2363, merged at 21:29:32 as `b9cbba8f400e42a78bc58e1ef502f46de5b89d7c` through normal checks and merge queue. PM reports exact merge-group CI 41915284100 green. Bridge stays 1. This task granted no PR approval.

**Source boundary.** The #2363 result also changes Rust and Cargo (native title bar, client size, notification labels). Binary v4.6.78 lacks those changes. This proof shows page adoption only. It does not show the native fixes, which need the normal binary release.

## A3 Publication to adoption

PM pushed `ui-live`, and run 41916713527 published bundle `1793827248-b9cbba8f400e`. No manual check was pressed: the tray's own 15-minute poll fired. No debug flag or local publication override was used. `timing.json` holds the values and `store-samples.jsonl` holds the store samples.

| Step (Windows) | Time | Seconds after pointer |
|---|---|---|
| Pointer asset created | 21:30:49 | 0 |
| Public read still old | 21:31:03 | 14 |
| Public read new | 21:31:32 | 43 |
| Check starts (first scratch folder) | 21:36:18.685 | 329.685 |
| Bundle staged in the store | 21:36:23.354 | 334.354 |
| First reloaded Conversations sample | 21:36:26.12 | 337.116 |
| `ui/state.json` current is the new bundle | 21:36:33.153 | 344.153 |

Bottlenecks, largest first:

1. **Poll wait: 329.685 s, 96% of the Windows total.** The wait depends on where publication falls in the 15-minute cycle, so it can reach about 900 s. The Mac fixture measured 773.788 s for the same publication.
2. Publication: 53 s from merge to the `ui-live` run (PM push), then 24 s from run creation to the pointer asset. Public reads stayed old for at least 14 s after the asset time.
3. Check to commit: 14.468 s. This has 4.669 s of download and verification, at most 2.77 s to the Conversations reload, and the 5 s clean-start trial.

No idle delay or 15 s retry was visible, because the last real input was older than 10 s. The hidden menu's own switch time was not sampled. The store commit is not attributed to a particular page.

Mac fixture, reported by PM and not repeated here: check 21:43:42.788, staged 21:43:45.849, commit 21:43:52.670. Check to commit took 9.882 s and publication to commit took 783.670 s. Proof: [native-proof.md](https://github.com/taskqorg/taskq/raw/abee80d2fd4f4c02f6441028e78393a127ffadc2/2390/8a730fcefdfc-native-proof.md) (sha256 `8a730fce…`), [after-adoption.json](https://github.com/taskqorg/taskq/raw/0a0077431b18cefb0c3a4b07143cda1c14b340d7/2390/98a5614e0e73-after-adoption.json) (sha256 `98a5614e…`); full hashes are in `timing.json`.

## A4 Retention on WebView2

Retained:

| Item | Before the check | After adoption |
|---|---|---|
| Service, tray | 41960 (19:50:55.158), 52132 (19:51:14.151) | Same PIDs and start times at 21:45:25 |
| Binary | v4.6.78, `58a66672…` | Same version and hash at 21:37:49 and 21:45:25 |
| Codex workers, descendants of the service | 43820 (21:31:41.900), 53964 (21:35:28.228) | Same PIDs and start times at 21:45:25 |
| This Claude worker | 54440 (21:30:41.064) | Same |
| Draft | `taskq#2390 controlled unsent draft - do not send` | Identical in every sample from 21:35:48 to 21:37:30 (`conversations-watch.log`) |
| Selected conversation | taskq#2390 | Same in the store and on screen |

Worker limits:

- Task mapping for the Codex PIDs was not read from the service. Only the menu ages at 21:42 (#2366 11 min, #2395 7 min) fit their start times.
- The ancestry of Claude 54440 runs through WmiPrvSE, so it is not shown as a service descendant.
- Baseline Codex PIDs 45244 and 50120 ended before this measurement and say nothing about adoption.
- Claude 45824 started after adoption and had gone by 21:45:25. It was not established.
- Linux and Mac production workers were not measured.

No replay: the store shows `op` and `receipt` null, and no board comment holds the draft. The only match for its phrase is PM's earlier Mac note.

Not proved:

- **Composer attachment.** A real click on Attach at about 21:33:36 opened a picker. Setting its file name and posting OK produced no chip and no stored file. The dialog stayed hidden and hung. A second click at 21:39:11, after adoption, gave the same hidden hung dialog. So the selected composer had no attachment during adoption. The cause was not established. An unrelated `ctfmon.exe` Fail Fast dialog was on the desktop and was left alone.
- **Exact reading position.** The viewport was not pixel-identical. Before the reload, the about 80 px viewport showed a recorded image. After it, the viewport showed the following "6 steps · 6 commands" row. Worker history moved from y -2298 to -2072, and its height fell from 4150 to 3514 px. The header also grew about 16 px. The stored anchor after adoption is `{at:1544207,i:0,y:14}`; its value before adoption was not read. Position stayed in the same area of the thread but drifted, and the cause was not established. Compare `native-conversations-before.png` and `native-conversations-adopted.png`.

Finding, seen once and not retested: Conversations reloaded with the first picker still open and its owner window disabled. The page's idle rule in `src/workspace.js` considers only composing state and the last input time. A file being chosen at that moment might be lost, and the window can stay disabled.

Partial support only: the pre-existing disposable draft `qualify-2022-retained-draft` has a stored attachment. Its record is identical at 21:35:49 and 21:42, and its stored file still matches hash `d05a62a37a6167c063274eae6354595a2a53f25e5c4c4327da081e0cebe3b788`. It was not the selected composer, and its chip was not observed on screen.

Mac fixture, reported by PM on WKWebView: draft and attachment hashes unchanged, visible marker kept, same PID 68556, thread kept. That fixture has no service or workers, and caret and focus are not claimed. It does not replace WebView2.

## A5 Native images

Each image comes from the real window through `PrintWindow`, and each was inspected.

- `native-conversations-before.png`: old page, header `taskq · #2390`.
- `native-conversations-adopted.png`: new page, header `taskq#2390`, same draft.
- `native-menu-adopted-sanitized.png`: new menu with `taskq#2390` style identifiers, footer TaskQ v4.6.78, captured at 21:42:33. The host row is masked and rows below taskq are cropped.
- `native-about-adopted.png`: `Interface 1793827248-b9cbba8f400e · adopted`, `Tray installed 4.6.78`, `Service 4.6.78`, captured at 21:43:33.

Linux native pages remain unknown. The Mac fixture exported no images.

## Actions on the owner's real tray

The proof needed these actions, and the owner should know about them:

- Two synthetic tray clicks. UI Automation opened the menu, Conversations, Settings and About, and it set the draft and the search field.
- Real wheel input, foreground switching and three real clicks. Two clicks were on Attach. The third click landed on the list view because the thread had not opened, and it collapsed the `serverq` group. That group was expanded again.
- Stopping the two picker helper processes, 50508 and 40340, which my clicks had created, and re-enabling the Conversations owner window that the first picker left disabled.

Tray and service were never restarted. Channels and logins were not changed. Send was never pressed. The cleanup left the draft and search empty, selection back on `serverq#47`, Settings and Conversations closed, and the samplers stopped. `ui/state.json` stays on the adopted bundle, as normal publication intends.

## Reproduce

Run from the project worktree with the existing host credential. No login, debug port or override is needed. Keep the install's normal channel.

```powershell
Get-FileHash -Algorithm SHA256 -LiteralPath "$env:LOCALAPPDATA\taskq\releases\4.6.78\taskq.exe"
gh api repos/taskqorg/taskq/releases/tags/v4.6.78
node docs/evidence/2390/store-sampler.cjs store-samples.jsonl
powershell -File docs/evidence/2390/conversations-watch.ps1 -Hwnd <Conversations window handle> -Log conversations-watch.log
gh api repos/taskqorg/taskq/releases/tags/ui-channel --jq '.assets[]|select(.name=="ui-latest-b1.json")|.created_at'
gh api repos/taskqorg/taskq/actions/runs/41916713527 --jq '{event,head_branch,head_sha,created_at,conclusion}'
git show -s --format=%ct-%H b9cbba8f400e42a78bc58e1ef502f46de5b89d7c
```

Start the samplers, let PM publish, and wait for one poll. Then read About and `ui/state.json`. To read the embedded identity, scan the binary's bytes for a JSON object beginning with `schema: 1` followed by `bundle`, and decode it with Python's `json.JSONDecoder.raw_decode`. The process list comes from `Get-CimInstance Win32_Process`, keeping only PID, parent PID, executable name and start time.
