# UI bundle

Tray and Conversations pages ship separately from the binary, so a display change reaches a host without a binary build, a service restart or a worker interruption.
The bundle replaces no native part: windows, tray icon, notifications, the IPC bridge and the service stay in the binary. It is not a hosted site: pages are files on the host, conversation text stays on the host.

## Contract detail

**Bundle.** One directory of four files: `manifest.json`, `tray.html`, `workspace.html`, `operations.json`.
`manifest.json` holds `schema`, `bundle`, `bridge` and each file's SHA-256 and size. `bundle` is `<commit epoch>-<12 hex of the commit>`; a higher epoch is newer, ties go by string order.
Each page is one self-contained document with inlined scripts and styles. `scripts/build-web.cjs` writes the bundle, and every binary embeds the bundle built from its own commit.

**Bridge rule.** `bridge` is one integer in `src/ui-bridge.txt`. A bundle is compatible only when its `bridge` equals the binary's.
Bump it in the same change when a native message or snapshot field that an older page or binary depends on changes incompatibly; additive fields do not bump it.
Binaries of an older bridge keep following the pointer for their own bridge, so they never see a bundle they cannot run.

**Publication.** The PM pushes a merged `main` commit to `ui-live`. `release.yml` then runs only `publish-ui`: the same release check as a binary release, then `node scripts/build-web.cjs`, then prerelease `ui-<bundle>` with assets `taskq-ui-<bundle>-<file>`, `SHA256SUMS` and an attestation.
Last it replaces `ui-latest-b<bridge>.json` on the fixed prerelease `ui-channel`. Neither release is ever marked latest, so `releases/latest` stays on the binary release and its assets, names and `SHA256SUMS` do not change.
Trust is CoreQ R11: HTTPS to the repository and who can push to it; no signature is added.

**Store.** `<install>/ui/<bundle>/` holds downloaded bundles; `<install>/ui/state.json` holds `current`, `previous` and `bad` (bundle to reason).
A download lands in a temporary directory, every file is checked against `SHA256SUMS` and the manifest, and only then is it renamed into place. Other bundles are pruned.

**Check.** A binary built by `release-build.yml` (`TASKQ_UI_CHANNEL`) checks at start, every 15 minutes, on the Update action and on About's Check for interface update. A development build keeps its own pages.
Network failure stays in About as the last check error, not in the errors list: the running copy is still good.

**Adoption.** After a check brings a newer compatible bundle, each open page is asked to adopt it, and asked again every 15 seconds until it does. Adoption does not raise or focus a window.
Conversations uses the safe Reload path: wait for 10 seconds without key or pointer input, flush, decline while a draft save, command receipt or failure is pending, checkpoint draft, attachments, selection and reading position, recreate the webview, restore. An interrupted operation is reported, never replayed.
The menu adopts while hidden or pinned; Settings and Views adopt while hidden. None of them adopts while an input holds an edit or a confirmation is open. A hidden or closed page takes the new bundle the next time it is built.

**Fallback.** Order is `current`, `previous`, embedded; the newest that verifies wins. A bundle fails when its manifest, a checksum or its bridge is wrong, when a page does not report ready within the existing 30 second acknowledgement limit, or when a page faults within 5 seconds of ready.
A failure records the reason under `bad`, so the bundle is not retried, reverts to the previous good or embedded copy and shows the reason in the errors list until a newer good bundle replaces it.
A newly adopted bundle stays on trial until a page on it has stayed clean for 5 seconds after ready; only then does `current` move.

**Page fence.** Every webview starts with its one document and refuses every later navigation: remote URL, `file:` path or another document. New windows are refused.
A Content-Security-Policy injected by the binary allows only inlined code and `data:`/`blob:` images and fonts, and denies connections, frames, objects, forms and base changes. The IPC handler drops a message whose source is `http`, `https` or `file`.
There is no local HTTP server and no remote page in a bridged webview.

**Display logic.** New display logic (labels, order, grouping, age and count text, row shaping) belongs in the bundle, and Rust sends raw facts. Rust keeps what needs native access or must hold when no page runs: state collection, command execution, the tray menu and notifications.

## Proof

`scripts/ui-bundle-proof.cjs` runs a candidate tray, its service and a stand-in worker from a private install and reads them over
WebView2's debugging port: a local publication, adoption, the kept draft, the page fence and each fallback, with pictures.
WKWebView has no such port, so `--store` writes an adopted bundle whose page tries to leave for a remote URL, and
`TASKQ_UI_USE_STORE=1 taskq tray --snapshot` pictures menu and Conversations from it. Both hooks exist only in debug builds.

## Where display logic still lives

Baseline for 174 commits touching web files in `src/`, 2026-10-05 to 2026-11-04, measured by diff hunk rather than by file because `tray.rs` holds inline tests:

| Commit also changed | Commits | Bundle-only with this design |
| --- | --- | --- |
| Nothing else | 43 | yes |
| Production display code in `tray.rs`, `conversations.rs` or `display.rs` | 28 | only after the move below |
| Tests of that code only | 1 | yes |
| Other Rust | 102 | no, or only partly |

Most-changed production functions in those display files, in hunks: `tray::run` 410, `tray::project` 49, `tray::state` 48, `tray::shots` 24, `conversations::act` 23, `tray::refresh_icon` 17, `tray::act` 17, `task_session` 15, `ai` 15, `answer_args` 14, `conversations::dialogue` 13, `summary` 13, `shot_name` 12, `answer` 12, `resource_group` 10, `conversations::snapshot` 9, `display::model` 5.

For the bundle to help, this view shaping must become raw facts with the formatting done in the page: `state`, `project`, `summary`, `task_session`, `resource_group`, and the labels, order and age in `display::model`. `run` and `act` stay native: they are the event loop and the commands.
Those moves are follow-up tasks. The first release moved only bundle status: Rust sends `ui` facts and the page writes the text.
