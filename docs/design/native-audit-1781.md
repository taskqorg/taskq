# Native design audit · #1781

[#1781](https://github.com/taskqorg/taskq/issues/1781) checks the native menu and question screens against the approved [conversations-preservation](exploration/conversations-preservation-audit.md) prototype renders in [conversations-input-scroll-2026-10-28](review-evidence/conversations-input-scroll-2026-10-28/), after [#1775](native-audit-1775.md).

## Method

- Native: the Windows branch build `taskq tray --snapshot` (real WebView2) on a `docs/shots/fixture.sh` install. Only the Mac `dev` service installs dev builds, and only from merged `main` (§ Shared core 1), so this Windows host on `live` cannot run an installed dev build of the unmerged branch; the installed pass follows the merge.
- Window sizes follow the installed defaults: the menu at 320 px (1x), Conversations at 800x600. This host's Windows Text size (140 %) scales WebView2 but not the window, so at 18 px the page is 572x429 CSS px: the conversation list is behind its toggle.
- `answers` snapshots (#1781) add one fixture project with an open question (`tests/fixtures/question-menu.js`) to the real state: Home, then that question's details. `conversations` snapshots use fixture threads; light adds the question (empty answer) and chat screens, so the set stays at ten screens.
- Target: the prototype renders are 1440x1024, dark only; each pair crops the menu panel or the Conversations panel. Light native screens pair with the dark target: the prototype has no light render.
- An item passes only when the native screen shows it as the target does.

Evidence: [native-1781-2026-10-29](review-evidence/native-1781-2026-10-29/) (native left at 1x, target right).

## A1 screens and targets

| Screen | Target | Evidence |
|---|---|---|
| Tasks menu with an open question | input-scroll 04 menu panel | `01`, `02` |
| Question task details | input-scroll 05 menu panel | `03`, `04` |
| Question in Conversations | input-scroll 04 Conversations panel | `05`, `06` |
| Conversations chat | input-scroll 09 Conversations panel | `07`, `08` |
| Filter chips, host strip, task rows, Settings | input-scroll 07–09, #1660, #1661 | #1775 [installed pass](review-evidence/native-1775-installed-2026-10-29/) |

## A2 checklist

| Item | Target | Native | Fix |
|---|---|---|---|
| G1 Ask row state icon is an amber question glyph that opens the question in one click | 04 | Pass (`01`, `02`) | `3c86d8d2` |
| G2 Question context (task title, why asked, Done and evidence) stays visible with the question | 04 | Pass (`05`, `06`): the thread title names the task; state, Done, the question and its options sit above the composer with no scrolling; the #1787 Evidence disclosure follows them | `3c86d8d2`, this change |
| G3 Task metadata disclosure reads `Task info` with the info icon | 04, 05 | Pass (`03`, `05`) | `3c86d8d2` |
| G4 Detail controls are icon buttons; the board link is the provider icon (`Close #n on the board`) | 05 | Pass (`03`, `04`) | `3df8081d` |
| G5 Question details are one neutral card with an amber edge and title; the Open question icon is amber | 05 | Pass (`03`, `04`) | this change |
| G6 Menu details disclosure reads `Task info`, not `Details` | 05 | Pass (`03`) | this change |
| G7 Question thread: Task info above the title; the title row holds board, Mark read and Archive icons | 04 | Pass (`05`, `06`) | this change |
| G8 Answer composer: Send is the arrow icon; an empty draft shows no validation line | 04 | Pass (`05`, `06`) | this change |
| G10 Answer composer: `Or type an answer` placeholder, history icon bottom-left, info icon right of Send; history fills the draft only and the text-only note sits behind the info icon | 04 | Pass (`05`, `06`) | this change |
| G9 The ask glyph sits on a quiet chip, so it reads as a button | 04 | Pass (`01`, `02`) | this change |
| Project row is board icon, name, folder icon | 09 (newer than the 04/05 row) | Pass (`01`) | none |
| Options with the recommended mark, free-text answer | 04 | Pass (`05`, `06`) | this change |
| Chat composer, history and info icons | 09 | Pass (`07`, `08`) | none |
| Chat list beside the thread | 09 | Narrow window: list behind its toggle (`07`, `08`) | none |
| Host chips read three letters (longer on a shared prefix), the full name in the tooltip | 07 | Pass (`01`, `02`) | this change (restores #1729 after #1775) |
| Filter chips, task row state icons, Settings | 07–09 | Pass in #1775 installed pass; unchanged here | none |

Kept: the `Agent asks you` journey card (approved in #1773); its state now leads the Done line (#1787 card order) and the number and title move to its accessible name, as the thread title above shows them.

The #1775 `10-question-dark` mismatch was G1, G2, G7 and G8.
