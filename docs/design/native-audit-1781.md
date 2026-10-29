# Native design audit · #1781

[#1781](https://github.com/taskqorg/taskq/issues/1781) checks the native Mac menu and question screens against the approved [conversations-preservation](exploration/conversations-preservation-audit.md) prototype renders in [conversations-input-scroll-2026-10-28](review-evidence/conversations-input-scroll-2026-10-28/), after [#1775](native-audit-1775.md).

## Method

- Native: the branch build `taskq tray --snapshot` (real WKWebView, real install data). The Mac dev install is refused by the worker sandbox, so the screens come from the branch binary, not an installed build.
- `answers` snapshots (#1781) add one fixture project with an open question (`tests/fixtures/question-menu.js`) to the real state: Home, then that question's details. `conversations` snapshots use fixture threads and render dark only.
- Target: the prototype renders are 1440x1024, dark only; each pair crops the menu panel or the Conversations panel. Light native screens pair with the dark target: the prototype has no light render.
- An item passes only when the native screen shows it as the target does.

Evidence: [native-1781-2026-10-29](review-evidence/native-1781-2026-10-29/) (native left at 1x, target right).

## A1 screens and targets

| Screen | Target | Evidence |
|---|---|---|
| Tasks menu with an open question | input-scroll 04 menu panel | `01`, `02` |
| Question task details | input-scroll 05 menu panel | `03`, `04` |
| Question in Conversations | input-scroll 04 Conversations panel | `05`, `06` |
| Conversations chat | input-scroll 09 Conversations panel | `07` |
| Filter chips, host strip, task rows, Settings | input-scroll 07–09, #1660, #1661 | #1775 [installed pass](review-evidence/native-1775-installed-2026-10-29/) |

## A2 checklist

| Item | Target | Native | Fix |
|---|---|---|---|
| G1 Ask row state icon is an amber question glyph that opens the question in one click | 04 | Pass (`01`, `02`) | `3c86d8d2` |
| G2 Question context (task title, why asked, evidence) is always visible above the options | 04 | Pass (`06`) | `3c86d8d2` |
| G3 Task metadata disclosure reads `Task info` with the info icon | 04, 05 | Pass (`03`, `06`) | `3c86d8d2` |
| G4 Detail controls are icon buttons; the board link is the provider icon (`Close #n on the board`) | 05 | Pass (`03`, `04`) | `3df8081d` |
| G5 Question details are one amber-edged card; Open question is an icon | 05 | Pass (`03`, `04`) | this change |
| G6 Menu details disclosure reads `Task info`, not `Details` | 05 | Pass (`03`) | this change |
| G7 Question thread: Task info above the title; the title row holds board, Mark read and Archive icons | 04 | Pass (`05`, `06`) | this change |
| G8 Answer composer: Send is the arrow icon; an empty draft shows no validation line | 04 | Pass (`06`) | this change |
| G9 The ask glyph sits on a quiet chip, so it reads as a button | 04 | Pass (`01`, `02`) | this change |
| Project row is board icon, name, folder icon | 09 (newer than the 04/05 row) | Pass (`01`) | none |
| Options with the recommended mark, free-text answer | 04 | Pass (`06`) | none |
| Chat list, composer and info icon | 09 | Pass (`07`) | none |
| Filter chips, host strip, task row state icons, Settings | 07–09 | Pass in #1775 installed pass; unchanged here | none |

Kept: the `Agent asks you` journey card (approved in #1773). Not added: history and info icons in the answer composer; answers have no command history.

The #1775 `10-question-dark` mismatch was G1, G2, G7 and G8.
