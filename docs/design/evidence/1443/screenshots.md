# Eight distinct gaps for owner review

Owner direction updates A1/A2: at most ten screenshots, one per distinct gap.
This set contains eight. Passing matrix cells have text verdicts only.
[Image hashes and source identity](screenshots.json) bind these retained files;
none is an after-fix or native acceptance image (R12).

| Gap | Visible issue / measured evidence | Screenshot | Brief |
|---|---|---|---|
| G01 | Back leaves the viewport when the project scrolls; y 45 → −4750 CSS px. | [Scrolled project](gaps/G01-back-scroll.png) | H |
| G02 | At 200% text, Close starts at x 329.31 in a 320 px viewport and is clipped. | [Clipped Close](gaps/G02-clipped-close.png) | H, T |
| G03 | Fine-pointer Home is 25.67×27.59 px; count capsules are 28 px high; resize is 18×18 px. | [Small targets](gaps/G03-small-targets.png) | T |
| G04 | The route title is outside the fixed header; title type is 20 px/700 rather than 22 px/600 at b=20. | [Type hierarchy](gaps/G04-type-hierarchy.png) | H, S |
| G05 | Project identity and task/card headings repeat without distinct context. | [Repeated identity](gaps/G05-repeated-identity.png) | L |
| G06 | Empty Attention still occupies a Home summary row. | [Empty Attention](gaps/G06-empty-attention.png) | L |
| G07 | Quiet Closed text is 4.32:1 against its fill, below the 4.5:1 target. | [Text contrast](gaps/G07-text-contrast.png) | S |
| G08 | A received workspace update loses Refresh focus to BODY; ordinary updates have zero live/status regions. The image shows context; the DOM probe establishes these gaps. | [Workspace update](gaps/G08-workspace-update.png) | U |

## H

Header/Back: [G01 Back](gaps/G01-back-scroll.png), [G02 Close](gaps/G02-clipped-close.png), [G04 title](gaps/G04-type-hierarchy.png).

## T

Targets: [G03 hit boxes](gaps/G03-small-targets.png), [G02 clipping](gaps/G02-clipped-close.png).

## S

Spacing/type/contrast: [G04 hierarchy](gaps/G04-type-hierarchy.png), [G07 contrast](gaps/G07-text-contrast.png). Spacing measurements remain in the findings table; no extra variant is committed.

## L

Lists: [G05 repeated identity](gaps/G05-repeated-identity.png), [G06 empty summary](gaps/G06-empty-attention.png).

## U

States/focus: [G08 workspace update](gaps/G08-workspace-update.png). The probe, rather than a still image, establishes focus and announcement behavior.
