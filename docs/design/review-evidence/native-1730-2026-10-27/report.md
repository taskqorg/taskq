# #1730 A6 native Mac View evidence (task #1733)

- Candidate: PR #1732 head `6534d60391b6f41be64a6b2effd2233c6d030e7e`. UI sources (`src/task-dashboard.js`, `src/task-views.js`, CSS) are unchanged from that commit.
- Binary: release build of the candidate plus the evidence-only [views-fixture.patch](views-fixture.patch) (snapshot scenes in `src/tray.rs` only; not for merge). Cargo version 4.6.39; SHA-256 `61967cf8f4a55470efefd72a99d9f3d7fd1eb193219ea973b92665b6b9d4f191`.
- Host: macOS 27.0.1 (26A434), Apple Silicon Mac, 2026-10-27.
- Install: isolated `TASKQ_INSTALL` directory with eight invented projects from [fixture8.sh](fixture8.sh) (every task state present). The owner installation, its service and its saved-view store were not used or written.
- Command: `TASKQ_INSTALL=<fixture>/install TASKQ_INCIDENT_REPORTS=off taskq tray --snapshot <out> --route views`; exit 0 (the snapshot run exits 1 on horizontal overflow).

## What "native" means here

The screens are rendered by the macOS system WebKit (`WKWebView`) inside the candidate tray binary at the View window size, 480x600 logical points, 2x backing (960x1200 px). This is the same renderer and page as the installed View window. It is not a window-server screen capture: native window chrome, window position and real pointer input are **unknown**. Screen Recording is not granted to workers, so no TCC prompt was triggered or dismissed. Interaction (Save as…, typing, scroll) is scripted in the page, not done with a real pointer.

Browser proof for the same candidate (Playwright, Chromium) is in PR #1732 `docs/design/evidence/1730/`. That proof is separate from this package.

## Screens

| # | File | Text size | Scene | SHA-256 |
|---|---|---|---|---|
| 1 | [01-views-16pt.png](01-views-16pt.png) | 16 pt | Three saved views (All tasks, Completed, Review work *), All projects + 8 projects, All states + 8 states | `b6a2e54c3de23830ae7f3ebf5ffcb8c32561880cbc8e8c7b7fcca28633bb8f5c` |
| 2 | [02-naming-undo-16pt.png](02-naming-undo-16pt.png) | 16 pt | "Deleted My filters" with Undo; Save as… naming row with Create and Cancel | `209140e038deb8ef174da824e84ba8792c4b22af2427925ad79733e26190c639` |
| 3 | [03-long-20pt-top.png](03-long-20pt-top.png) | 20 pt | Long unbroken view, deleted-view and project names; content scrolled to top | `194fc88dd72feb6a97ea48c4b1af1de067e6131d61ba596c88fb63ffd7e2a7ee` |
| 4 | [04-long-20pt-end.png](04-long-20pt-end.png) | 20 pt | Same, content scrolled to the end | `870264521ef4e2ba9ec4f57a1cad82feebca09e5815c9f429ad9058334600c52` |

## Results

| Item | Result | Screens |
|---|---|---|
| A1 candidate and binary identified | pass | header |
| A2 three saved views and all eight project and state filters visible at 16 pt | pass | 1, 2 |
| A2 naming and Undo | pass | 2 |
| A2 20 pt long-name overflow | pass, with finding F1 | 3, 4 |
| A3 Close fixed in the header; Display and Save / Save as… / Reset / Done fixed in the footer | pass | 1–4 |
| A3 one content scroll on overflow; no horizontal scroll | pass (snapshot overflow check exit 0) | 3, 4 |
| A3 no clipped essential control | pass, with findings F1, F2 | 3 |
| A4 owner saved views unchanged | pass: hash equal, 3 views before and after | none |
| A5 native vs browser distinction | this report | none |

**A4.** The owner saved-view preference was hashed as canonical JSON (sorted keys) before and after the capture. The value is `ae449bbe873d29bb541fe7f631a6f2b2c14c9cf8eb055d66c1caa7257997a99d` both times; the view count is 3 both times. The content is not published.

**F1 (minor).** At 20 pt with long project names, the "All projects" cell stretches to the height of the tallest project cell. Its label is then not visible in the first viewport (screen 3: empty selected box at the top left of Projects). The control is still there and selectable; the label shows only after scrolling.

**F2 (minor).** At 20 pt the header title "View · <long name>" is cut at the window edge without an ellipsis (screens 3, 4). The full name stays readable in the saved-view row.

**Note.** No per-filter count badges render next to project or state filters in this candidate. Not required by A2; recorded for review.
