# README simplification

The owner approved an installation-first README with three interface examples and optional commands. [Final page](after.html), [pre-edit page](before.html), [README source](../../../../README.md).

## Scope and provenance

The page follows one illustrative storefront change: draft a cart/sign-in request, follow work and redirect review, answer the cart worker's quantity question. Long dialogues, telemetry tables, runtime-switch logs and the second illustration are removed. Setup, new-project instructions, CLI access, release verification, MIT and donate remain.

The Product decision and supporting page plan changed before publication under R13. The change is tracked in [#1921](https://github.com/taskqorg/taskq/issues/1921). Pipeline research is the separately running [#1919](https://github.com/taskqorg/taskq/issues/1919). No production UI source, installed service or real board task was used for capture. Unrelated changes in the original working tree are excluded.

Publication is rebased on `516af799a8806031f27d37ed816d353bcd700aa0`, release 4.6.47. The current PM-chat fixture, transcript stub and empty-answer reset are preserved. The three native images were regenerated with this version's actual macOS WKWebView renderer. Source identities, binary, capture inputs and image hashes are in [the manifest](../../manifest.json). Data is invented; requests and answers remain unsent drafts. Screenshots do not prove task creation, delivery or completed work.

The capture script builds once and reuses the existing macOS signing runner for both routes. The Windows branch retains direct binary execution and is untested here. The initial 4.6.39 images were reviewed in the parent checkout; the pre-edit rendering retained here is that local baseline, not a capture of current GitHub main.

## Visual evidence

All three final native images are inspected individually. Request text and send control, all queue rows and lower controls, and the complete source-numbered question/answer/send area are visible. The current UI's unknown observations remain explicit. Current-release chrome and Reports/sidebar controls are preserved rather than reconstructed from a prototype.

[Desktop page](after-960.png), [narrow page](after-390.png), [expanded narrow page](after-390-expanded.png), [viewport checks](page-checks.json). The HTML is GitHub Markdown API output with the retained local stylesheet, not a capture of GitHub's website. Wide Conversations images are overviews on narrow screens and link to their readable originals. Code blocks scroll internally.

## Checks

The native capture command, screenshot freshness check, image/capture-input hashes and whitespace check pass. Browser checks pass at 960 and 390 px: every image loads, no horizontal page overflow, and all three disclosures open. The final page renders and all three native screenshots were opened and visually inspected. Full-gate failures in the original mixed working tree were historical observations and do not qualify this isolated candidate. No merge, release or deployment is performed by this publication.

The selected gate `python3 scripts/check.py --base 516af799a8806031f27d37ed816d353bcd700aa0` passed on code commit `f3be11ae06c4842a8fefa0487dc35fb89a352441` in 197.293 seconds: 693 Rust unit tests and 21 integration tests passed, offline selftest and Node/artifact-script checks passed. The suite ignored 35 unit and one integration case under its normal policy. This final documentation-only follow-up records those results; source, fixtures, images and dependencies are unchanged. GitHub CI and merge acceptance are separate.
