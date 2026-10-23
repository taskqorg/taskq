# Workspace dialogue UI · #1665

Builder evidence for the D2–D7 workspace surface of a launched conversation: reply composer, transcript view, state and Status control. Not owner acceptance, a merge or a deployment. The backend turn, reader and watch service are #1664.

## Candidate

- Sources: `src/conversations.js`, `src/conversations.css`, `src/conversations.rs` (`dialogue` reads), `src/tray.rs` (`conversation-dialogue` dispatch off the event loop), `src/workspace.js` (Reply in Conversations from the message page).
- Environment: Windows 11, Playwright Chromium headless shell, scale 1; fixtures at 16 px, dark and light, 900×720 and 350×560.
- Browser fixtures answer with invented native receipts; no native turn, board or watch effect runs. Rust tests use isolated stores.

## Proof (D-A, source and native apart)

Source: `node tests/dialogue.browser.cjs` (UI over fixture receipts), `node tests/conversations.browser.cjs`, `cargo test --locked --bin taskq conversations::tests` and the #1664 `dialogue::tests`. Native: a real Claude Code or Codex session driven from the workspace.

| ID | Claude source | Codex source | Claude native | Codex native |
|---|---|---|---|---|
| D-A1 send continues the session; replies only from the transcript | Pass: Send/Enter posts one `reply` for the launched ID, never `send`; accepted clears the draft; the reply appears only after the reader returns it | Pass: same path, accepted clears the draft | Not tested | Not tested |
| D-A2 refusals keep the draft | Pass: `working` and `open in claude; close it there first` show as `Not sent: …. Draft kept.`; Send is disabled while a reply waits | Pass: `open in codex; close it there first` keeps the draft | Not tested (one branch) | Not tested |
| D-A3 chunked view, infinite scroll, live append | Pass: newest chunk in order, tool calls one line, opens at the bottom; scroll-up and Load earlier prepend without jumping and reach the first record; new records append on the 2 s poll | Pass: unreadable transcript reads `Transcript unknown: …`, never empty; the view loads once readable. Scroll not exercised | Not tested (10 MiB) | Not tested |
| D-A4 Status and watch | Pass: `no watch`; set with prompt, every ≥ 5 and events in D6 order; prompt, every, events, last and next turn, last outcome and last assistant line shown; edit and remove; refused edit shows its reason | Pass: set from Status | Not tested (schedule, events, hold) | Not tested |
| D-A5 restart and unknown | Pass: state is `unknown` until a live read, and when the read fails; never idle or done. Restart: Not tested here | Pass: holder/transcript failure reads `unknown` in the row and the view; Rust read of a missing transcript is `unknown` with a reason | Not tested | Not tested |

## Screens

[transcript-dark-16](transcript-dark-16.png), [status-watch-dark-16](status-watch-dark-16.png), [refused-dark-16](refused-dark-16.png), [codex-unknown-light-16](codex-unknown-light-16.png), [narrow-light-16](narrow-light-16.png). Reviewed by the builder at natural size; no second review.

## Open

- Native rows above need a real launched session per runtime.
- At 350×560 the status footer can overlap the last composer line; `tests/conversations-render.browser.cjs` already fails its narrow composer check on this Windows host before this change.
