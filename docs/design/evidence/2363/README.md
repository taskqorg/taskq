# Native UI follow-up: evidence

Source: [owner request and screenshot](https://github.com/taskqorg/taskq/issues/2363). Applies R6, R7, R8, R12, R13 and section 7. Product and linked UI decisions changed first under R13. No host policy, active attempt, canonical task identity or task destination changes.

## Resize cause

The cause was established natively by [Mac blocker #2375](https://github.com/taskqorg/taskq/issues/2375); measurements and the correction are in [its evidence](../2375/README.md).

`wry` 0.57.0 replaces the window content view, while `tao` 0.37.1 answers `inner_size()` from its own detached view, so TaskQ reads a stale 640x800 physical size. The size handler computes `frame = outer - inner`, which then grows with the window. Above a natural height near 645pt, the fit has no fixed point and flips between two heights; the page refit on every native resize closed the loop.

There are two fixes, and neither uses timers or suppresses requests:

- `client_size` reads the content rectangle from the window frame on macOS, and is used at all three stale sites: fitting, the Fixed `size-anchor` save and native header drag.
- The page refits on width changes, not on height-only native acknowledgements. Content and font changes and intentional dimensions keep the existing fitting path.

## Native measurements (Mac, from #2375)

`installed-src` is the installed source, `parent` adds this task's width-only refit, and `fix` adds the native size correction. Rest is 15 s at the top anchor.

| Measurement | `installed-src` | `parent` | `fix` |
| --- | --- | --- | --- |
| Native resizes at rest | 888 | 0 | 0 |
| Main CPU at rest | 40.68% | 0.2% | 0.21% |
| WebContent CPU at rest | 21.65% | 0.02% | 0.02% |
| Native bottoms, content change, top anchor | 1122, 1722 | 1164, 1680 | 1776, 1912 |
| Native bottoms, content change, bottom anchor | 1218, 1626 | 1068, 1776 | 1912 |
| Native resizes, straddling fit | 658 (660 size requests) | 8 (8 requests) | 0 (8 requests) |
| Titlebar pixel, Dark at background 35 | not sampled here | 0,0,0,0 | 20,20,20,255 |
| Titlebar pixel, Light at background 35 | not sampled here | 0,0,0,0 | 236,236,236,255 |
| Body alpha at background 35 | not sampled here | 147 | 147 |
| Fixed mode saved size | not sampled here | 320x400 (wrong) | 400x644 |

The width-only refit removes the endless loop and its CPU. The native correction removes the height flip: at the bottom anchor the window bottom stays at 1912 through content changes.

Hit testing on the fixed window lands on the titlebar effect view, which can move the window. Before the fix, an HID drag fell through the transparent titlebar and moved another app's window. After the fix, drags moved the TaskQ window, native Close reported `close_requested`, the resize grip moved 350x616 to 400x676, and `storefront#100` reached the real pasteboard.

Whole-window alpha applies to the whole native window: at 80% the titlebar and the body alpha both read 204.

## Browser checks

- `native-ui-followup.browser.cjs`: counters precede the toolbar including wrapped Held (A2). One leading List toggle, pressed for flat, keeps view and search (A3). Monochrome Antigravity in both themes (A4). Exact identifier and version clipboard contents, failure feedback, keyboard and touch reach, and no row action (A6, A7, A8). New invariant: the rendered Home and Conversations text, `title` and `aria-label` contain no bare `#N`.
- `resize-feedback.browser.cjs`: four height changes with resize acknowledgements produced six size requests on base `699ad95d` and zero on the candidate; width and content/font changes still fit. This is adapter evidence; the native proof is above.
- `tests/*.test.cjs` all pass, including a search test for a pasted `project#NNN`.
- Browser suite on Windows: 31 of 66 pass. The other 35 fail at the same `file:line` on a clean export of `origin/main` `07ebe3b5`, so none is a regression from this change. `scripts/check.py` runs only changed browser files, which is why they do not block the gate there.

Browser pictures: [dark Home](home-dark.png), [light Home](home-light.png), [Conversations](conversations.png). They show content rendering, not native titlebars, movement or runtime stability.

## Limits

- The Mac measurements ran on `1b237189` plus the correction patch, in a development-profile snapshot mode, and not on this merged tree or an installed build.
- The installed live build showed 0 bounds changes at rest with main-process CPU still 26.19%. Rest CPU is therefore not frame resizing, and its cause remains unattributed. The rest CPU figures above come from the snapshot mode, and no improvement is claimed for that installed state.
- Windows and Linux native frames and resize behavior were not measured. Only the macOS code path changes there.
- Whole-window drag was not run, and saved-position behavior was not exercised.
- `native_titlebar` depends on the current view hierarchy. If it stops matching, the app logs one stderr line and leaves the titlebar unchanged.

## Remaining identifier gaps

The owner asked for `project#NNN` in all UI. These paths keep other text:

- Host-authored receipt reasons (`src/dispatch.rs`) and outcome text inside notification bodies, such as `replaced by #104`, are free text carrying a bare `#N`. Changing them needs a host change, which this task excluded.
- CLI and log lines (`src/health.rs`, `src/profile.rs`) are not UI and are unchanged.
- A pin on an unavailable board shows its stored reference.
- `Task details for`, the single-task `Start` label and the dependency link in `src/task-dashboard.js` use the shared identifier, but no current row renders the `task-detail` trigger, so no browser test reaches them. The Held filter, Held admission review and Conversations Held thread are the reachable paths, and `native-ui-followup.browser.cjs` covers them with a scan of visible text, titles and accessible names.
- `tests/tray-state-filter.browser.cjs` and `tests/tray-answer.browser.cjs` fail at this task's base as well as on this branch, with the same assertions, and this task does not change them. `tests/reports.browser.cjs` failed the same way until this task moved its result opening to the current task row and host `open` message.
- Tooltips, accessible names, receipts, the answer-card link and the native Quit label use the format without a copy button. `Board` stands in when no project name is known. Long project names in the Quit label collapse to a `+N` count.
- Task titles are the owner's own text and are never rewritten.
