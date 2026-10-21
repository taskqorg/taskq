# Eight distinct gaps for owner review

Owner direction updates A1/A2: at most ten screenshots, one per distinct gap.
This set contains eight. Passing matrix cells have text verdicts only.
[Image hashes and source identity](screenshots.json) bind these retained files;
none is an after-fix or native acceptance image (R12).

| Gap | Visible issue / measured evidence | Screenshot | Brief |
|---|---|---|---|
| G01 | Back leaves the viewport when the project scrolls; y 45 → −4750 CSS px. | [Scrolled project](gaps/G01-back-scroll.png) | N |
| G02 | At 200% text, Close starts at x 329.31 in a 320 px viewport and is clipped. | [Clipped Close](gaps/G02-clipped-close.png) | N |
| G03 | Fine-pointer Home is 25.67×27.59 px; count capsules are 28 px high; resize is 18×18 px. | [Small targets](gaps/G03-small-targets.png) | N |
| G04 | The route title is outside the fixed header; title type is 20 px/700 rather than 22 px/600 at b=20. | [Type hierarchy](gaps/G04-type-hierarchy.png) | N |
| G05 | Project/task headings repeat; the task answer dialogue is inline in the menu. | [Repeated identity](gaps/G05-repeated-identity.png) | W, P |
| G06 | Empty Attention occupies a Home row above an inline menu composer. | [Empty Attention](gaps/G06-empty-attention.png) | W, P |
| G07 | Quiet Closed text is 4.32:1 against its fill, below the 4.5:1 target. | [Text contrast](gaps/G07-text-contrast.png) | N |
| G08 | A received workspace update loses Refresh focus to BODY; ordinary updates have zero live/status regions. The image shows context; the DOM probe establishes these gaps. | [Workspace update](gaps/G08-workspace-update.png) | W |

## Revised plan links

- [D — specification first](owner-plan.md#six-proposed-follow-up-tasks): owner requirements and source review; no invented UI image.
- [N — navigation and controls](owner-plan.md#six-proposed-follow-up-tasks): [G01](gaps/G01-back-scroll.png), [G02](gaps/G02-clipped-close.png), [G03](gaps/G03-small-targets.png), [G04](gaps/G04-type-hierarchy.png), [G07](gaps/G07-text-contrast.png).
- [W — workspace communication](owner-plan.md#six-proposed-follow-up-tasks): [G05 inline answer](gaps/G05-repeated-identity.png), [G06 menu composer](gaps/G06-empty-attention.png), [G08 focus/status context](gaps/G08-workspace-update.png).
- [O — workspace operation reviews](owner-plan.md#six-proposed-follow-up-tasks): source/owner requirement, not an image of a future view.
- [P — panels and task list](owner-plan.md#six-proposed-follow-up-tasks): [G05](gaps/G05-repeated-identity.png), [G06](gaps/G06-empty-attention.png).
- [M — messages/archive](owner-plan.md#six-proposed-follow-up-tasks): owner requirement; current archive UI/backend is absent, so no fabricated screenshot.
