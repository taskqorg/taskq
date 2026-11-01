# Conversations PM usability and recovery

Implements [#2159](https://github.com/taskqorg/taskq/issues/2159) from baseline `ba7652694e5ab3aa343ad7f1767d04b0f7c50aa8`. The issue and its owner steers retain the original request. Applied R13/TW4 and C2/C7/D4 before code, R9 for existing selected-runtime authority, R12 for proof boundaries and section 7 for publication. The accepted TW4 runtime choice already exists in the root contract's linked navigation target; its implementation and supporting runtime description are reconciled here without adding another root decision or changing a numbered rule.

## Before and diagnosis

[Checkpoint 1](checkpoint-1.md) records the installed baseline before edits and the shared native-window ownership. The worker's unsent test draft is `fc4d2c4ab05d1ab95b085df8590301b1`; the owner PM retains `0bbf213aace0afa5c7d26e6781b755f9`. The worker stopped production CUA navigation when the shared ownership became known. No worker PM launch or native update occurred.

The owner PM independently recorded the same saved draft/file surviving Reload and Close/reopen on `4.6.59-dev.1793573522`, while selection disappeared and reopen focused the Conversations heading (steers 6565318138/6565327134). The isolated actual-bundle Close/reopen check failed before with `native-shaped Close/reopen restores composer focus`. Navigation unconditionally focused the heading after the reader restored its own focus. Recovery could similarly override the saved field with the shell's Reload focus. Restoration now gives the selected thread's saved focus priority, and observes WebKit selection changes without requiring a click or keyup.

`conversations-cache.test.cjs` failed before because New retained `finished`; it now clears the result scope/page, preserves the new draft's explicit runtime and reveals that draft in Recent. The native baseline also showed a wrapped sidebar mode/count. The correction keeps those on one line; the Recent/Archive pair wraps together when required.

The runtime storage regression failed before with `explicit choice survives native storage` (`Null` versus `codex`). GUI runtime readiness previously covered only Claude, and the setup request boundary refused project Codex. The draft now saves its explicit choice, Send fences it against that saved revision, and immutable request readback retains the runtime. Missing/stale readiness stays unavailable; no fallback changes the selection. Unsupported choices preserve prior bytes. Fork and deletion recovery retain the independent draft's choice.

The owner also supplied the earlier natural failed-route sequence on `1793568201`: a route error became Interface error after Back, safe Reload yielded no window, and a useful menu appeared only after an independent update (steer 6565454109). Its original cause remains unknown. Source inspection found a separate definite loss path: Reload dropped the old WebView before constructing the next one, and creation failure dropped the retained native window. Main and detached readers now replace the view/generation only after successful construction. The isolated native-boundary test checks failure retains the old window/view/generation and success retains the window while requiring the new ordered recovery handoff. This is not an injected installed failure or attribution of the original episode.

## Candidate behavior

- New requests offer Claude Code and Codex, with separate fresh readiness and per-draft selection. Existing drafts retain the Claude default. Replies use the recorded session runtime; changing a draft never changes host defaults, grants, capacity, model or effort.
- Visible Codex uses the existing Codex app handshake/queue adapter and that project's existing `codex-app` worker grant. It runs in the exact project root; standalone setup keeps its owned folder. A CLI Workspace grant does not authorize an uncontained app turn. The owner PM has reported the existing taskq app grant as Full rev33; no grant is created or edited here.
- Close/reopen and recovery restore the selected field, range, file references and source context. Unknown receipts retain immutable IDs and require explicit reconciliation. Failed native recreation preserves the previous owned window and its recovery authority.
- Worker/native and result prose reuse the same safe vnode formatter for headings, emphasis, inline code, lists, code blocks and HTTP(S) links. HTML and unsafe links stay literal. Native source text remains unchanged; bounded pages and original record/generation/character anchors remain inspectable. Repeated launch transport copy was removed from empty drafts.

Formatting initially exposed an append-anchor regression: the deepest measured inline node and the first restored container had the same offset but different geometry. The unchanged `worker-panel.browser.cjs` guard failed. Restoration now chooses the deepest visible node at that offset, matching capture; that original guard passes. The protection was not weakened.

The conversation fixture also reached a legitimate second bounded page after creating its 65th item. Its old direct-row click timed out. It now revisits the original via Previous threads, preserving the original draft/file/one-launch assertions; no paging budget or production behavior was weakened.

## Selected verification

- `node tests/conversations-cache.test.cjs`: failed before for New/Finished; passed after, retaining stale-catalog and immutable submitted-body protections.
- `cargo test --locked --bin taskq owner_runtime_choice_is_saved_and_send_is_fenced`: failed before for discarded runtime; passed after. Unsupported choice and crossed Send preserve stored bytes and create no setup request.
- `cargo test --locked --bin taskq codex_project_request_keeps_its_explicit_runtime_and_identity`: passed on this Mac; exact project/runtime, same-ID readback, changed-runtime refusal and one fixture launch. The fixture launcher is substituted; no native launch is claimed.
- `cargo test --locked --bin taskq workspace_recreation_retains_identity_and_reader_on_failure`: passed for main/detached identities, failed construction and successful generation replacement. The WebView constructor is substituted, not OS-qualified.
- `node tests/conversations.browser.cjs`: passed per-draft Codex choice, immutable send, New/Finished, saved drafts/files/ranges, native-shaped Close/reopen focus, ownership and sidebar geometry. IPC is invented.
- `node tests/conversations-recovery.browser.cjs`: passed the existing 16-cell fault matrix and answer/steer no-replay checks, plus saved owner selection/focus/runtime recovery in Chromium and WebKit. Browser engines are not installed TaskQ windows.
- `node tests/workspace-route.browser.cjs`, `node tests/conversations-source.browser.cjs`, `node tests/worker-panel.browser.cjs` and `node tests/conversations-flow.browser.cjs`: passed failing-route safe Back, safe boot/handoff, exact source replacement, append anchor, immutable unknown delivery and native scrolling instruments.
- `node tests/reports.test.cjs`: failed before for literal emphasis/code; passed safe semantic formatting while keeping report receipt/decision guards.

A broad development `owner_` filter first failed an unrelated host refusal fixture under the launching worker's ambient task binding. That exact fixture passed with only its test subprocess bindings removed, as required by docs/testing.md; the parent worker remains bound. No host policy or assertion changed. The canonical gate runs its own isolated test environment.

The canonical candidate gate, exact-source independent review, performance run and installed PM journey are recorded in the next checkpoint; they are not implied by these selected checks.

## Review set

[Native before](before-sidebar.png), [explicit runtime choice](runtime-choice.png), [unwrapped sidebar count](sidebar-count.png), [worker document](worker-document.png). The last three are actual-bundle browser fixtures, visually opened after generation, not native OS captures. Theme/width variants remain instrument output and are not committed as extra review screens.

## Native handoff and limitations

The owner PM retains production GUI ownership and the single real Codex PM launch. Freeze the candidate commit before its native run. From its checkout, the existing `sh scripts/dev-install.sh <exact-candidate-SHA>` builds in the protected detached dev worktree, creates a new immutable signed version, runs staged selftest and uses normal host handover. The worker does not run it. Do not replace a live app file, stop the service, change grants/capacity or duplicate a launch. Record exact source, installed version and surviving worker identities before claiming native proof.

For layout-only iteration, `npm run build` followed by `TASKQ_EVIDENCE_1580=<ignored-output> node tests/conversations.browser.cjs` emits `preview.html` from the same bundle and invented state. It simulates local saves only; native and board actions are disabled. Use its file directly to inspect the draft/runtime layout without installing an app.

Keep the previous immutable version and every native draft/setup file for rollback through the normal handover. An older reader may refuse the new optional runtime field rather than discard it; do not delete or rewrite drafts to force a downgrade. Native chats and task state are not migrated.

A1 remains bounded by the before observations; the original installed failure cause remains unknown. A2/A3 installed after behavior, A5 same-PM clarification/result relay, A6 real report/navigation/settings journey and A7 installed Summary still require the PM's exact-source native rerun. Windows native qualification is not established by this Mac. Prior #2140 performance failures (100ms interaction budget and 32ms frame budget) and the unqualified long heap plateau remain explicit; no threshold was changed. Publication still requires green merge-result CI (R12/section 7).
