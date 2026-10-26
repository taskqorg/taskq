# Conversations #1723 verification

The candidate fixes M01–M03 and M06–M07 under C1/C2/C5/C7/D4. Applied R1 (navigation identities only, no task store), R13 (linked contract updated before implementation), R12 (browser, native rendering and installed-app observations remain separate), and § 7 (candidate checks and publication). No R-number changes.

## Failure and retained protection

Before the fix, `node tests/messages.browser.cjs` failed `first event-free task remains in Recent` (expected 1, actual 0). The same scenario now passes, including both identities after navigation rehydration. The menu dispatcher scenario opens Later, Doing, Review and Ask through exactly one task-thread entry without lifecycle effects.

The mixed-selection scenario covers task, event and owner selections, both Recent/Archive directions, empty detail, preserved nonempty drafts, and archived owner transcript with Restore before reply. It also caught a second defect: a snapshot reset expanded Status from scrollTop 2299 to 0. Capturing the rendered thread's position before repaint makes that scenario pass. Self-review also reproduced a task opening under the wrong project when two registered roots shared a board (`Other` instead of the explicitly requested `Target`). Navigation now retains the source root; the mixed-selection scenario protects it. New selection data uses the existing bounded navigation envelope; legacy owner selections and workspace answer navigation remain readable.

`tests/dialogue.browser.cjs` moves its existing read/anchor checks to the single detail scroll. Ordered chunks, prepend anchoring, append-follow, explicit unknown, reply refusal, watch validation and immutable operation checks remain. The render instrument now reaches Send through the detail scroll; it still checks complete framing and horizontal overflow. `tests/messages.browser.cjs` retains source choice numbers, recommendation, exact-revision card handling, explicit acknowledgement and independent owner/task drafts. Shared answer-card checks retain stale-question refusal and operation identity protection.

A local run also loaded the real #359 question from the previously retained native AX observation into the same browser scenario. Its source text, paths and screenshots are not published. This checks browser rendering of captured input, not the installed app or current board state.

## Visual review

Four synthetic browser screens are retained below, at 940 × 740, dark theme, system font. They show the selected archived owner, a long task question scrolled to its choices/composer, and expanded Status scrolled to the composer at 18 and 20 px. Intentional differences from the pinned reference are limited to #1723: one task identity heading, compact task metadata, coherent selections, archived transcript, and one thread-content scroll. Existing shell, palette, icons and list/detail proportions remain.

- [Archived owner, 20 px](archived-dark-20.png)
- [Long question, 20 px](question-dark-20.png)
- [Expanded Status, 18 px](expanded-status-dark-18.png)
- [Expanded Status, 20 px](expanded-status-dark-20.png)

These are browser instruments with invented receipts. They do not establish native delivery, installed-app operation or owner visual acceptance.

## Installed Mac journey (A8)

The read-only attempt to select the installed TaskQ app through the native UI tool returned `Computer Use server error -10005: timeoutReached` before an app observation. No native screen was captured and no live record was changed. A screenshot renderer is not substituted for this journey.

| Finding | Installed-app status | Reason |
|---|---|---|
| M01 | Not tested | Native UI observation unavailable |
| M02 | Not tested | Native UI observation unavailable |
| M03 | Not tested | Native UI observation unavailable |
| M06 | Not tested | Native UI observation unavailable |
| M07 | Not tested | Native UI observation unavailable |

Source/browser fixes are implemented. Installed Mac qualification remains unknown under R12; merge-result CI and deployment are separate from this report.
