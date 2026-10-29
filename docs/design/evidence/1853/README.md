# macOS Reports qualification (#1853 for #1826)

Mac part of #1826 A6 for draft [PR #1852](https://github.com/taskqorg/taskq/pull/1852). Evidence only: no implementation, no installed-host change, no live report decision.

## Candidate

- Source `f22d7b24758e9e1431f612c21a39d51a2123ba13`, the same source as the Windows frames of #1826. It is an ancestor of the #1852 head `f3a0ce0d`; later commits change evidence files only.
- Built in a detached scratch worktree outside the project: `npm ci`, `npm run build`, `cargo build --release --locked` (through `docs/shots/update.sh`). Release binary SHA-256 `e71b53df6d1898240d55ebe92b619fc570b6e32e6bcf1743a76c891015abd6b5`, signed by `scripts/macos-runner.sh`.
- Fixture: isolated `TASKQ_INSTALL` from `docs/shots/fixture.sh` (blob `fa252d0a`) with `tests/fixtures/reports-shots.js` (blob `458c3994`); report receipts are invented.
- macOS 27.0.1 (26A434), arm64, WKWebView, WebKit 22625.1.29.11.28. Display scale not measured.

[Mac manifest](mac-manifest.json) pins source, binary, fixture and image hashes.

## Frames (A1, A2)

`taskq tray --snapshot <dir> --route reports` and `--route reports-conversations`, run separately, 16 px:

- Menu: [dark](mac-menu-dark.png), [light](mac-menu-light.png), at the start of the reader.
- Conversations: [dark](mac-conversations-dark.png), [light](mac-conversations-light.png), scrolled to Make tasks, Later and Drop above the fixed footer.

Reports review set: #1826 has two before frames and four Windows frames; these four Mac frames make 10 of 10.

## Reader and actions (A3)

| Check | Observation | Scope |
|---|---|---|
| Conversations native frames | Make tasks, Later and Drop sit above the fixed `TaskQ v4.6.40` footer in both themes | WKWebView, pinned fixture |
| Menu native scroll | A scratch-only fixture line scrolled `[data-report=drop]` into view; all three actions sat above the fixed footer in both themes (image hashes in the manifest, frames not committed to keep the set at 10) | WKWebView, rebuilt binary `e0bed913…`; the fixture change was reverted |
| `node tests/reports.browser.cjs` | PASS: 16 cells and the native fixture bridge on both surfaces | Chromium headless shell instrument on this Mac, not native evidence; includes report and Conversations state restoration after collapse |
| `node tests/reports.test.cjs` | PASS | Component check at the pinned source |

Report state (open report, read marker, proposed task) stays visible in every frame. Native state restoration after collapse was not exercised in WKWebView; it is covered only by the Chromium instrument.

## Standard screenshots (A4)

Owner answer on #1853: run `docs/shots/update.sh`, record the outcome, do not commit README shots (taskq.md § 7 step 4 since #1847).

- `sh docs/shots/update.sh` at `f22d7b24` in the scratch worktree: passed. Its first run failed with `TASKQ_ROOT … is not the project of the current directory` because the worker environment leaked into the scratch tree; the rerun without `TASKQ_*` variables passed.
- `docs/shots/check.sh`: exit 2 (stale uiSources) before, exit 0 after.
- Output manifest: [shots-update-manifest.json](shots-update-manifest.json), SHA-256 `bb5a1bbb…52fa`, 30 uiSources blobs (`src/reports.js` `d78daaf0`). Images: `popover-dark-project-16.png` `4c176ddb…cda8`, `icon-summary-run-dark.png` `b723921e…d654`. They stay out of `docs/shots/`.

After #1852 rebases onto current main, `result` has no shots guard; the README shots workflow derives the shots at release.

## Unknowns

- Windows frames do not prove Mac behavior, and these Mac frames do not prove Windows behavior.
- Display scale, other fonts (only 16 px) and native collapse/restore on macOS are not measured.
- No canonical gate ran on `f22d7b24` here; #1852 still needs exact final candidate CI.
