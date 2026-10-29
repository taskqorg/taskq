# Conversations sidebar · #1808 stage 1

Proposal for owner selection, not runtime implementation. [CS1–CS6](../../tray-workspace.md#conversations-sidebar-proposal-1808-stage-1) specify the behavior; the [usability review](../../conversations-usability-review.md#sidebar-grouping-1808) records the baseline and scenarios. The [editable mockup](../../exploration/conversations-sidebar.html) embeds the actual v7 workspace bundle and replaces only its sidebar with an isolated invented fixture. Other product destinations are shown for context; this instrument does not qualify their behavior.

Recommendation: **Menu actions**. One permanent trigger per row preserves title width. The **Inline actions** comparison saves a click but reserves two targets and makes dense rows busier. Both keep the requested Pinned → projects → Other order. Approval of the selected variant also adopts CS1–CS6's stated edge cases; they remain proposals until the answer. No existing active rule or guarding assertion is changed.

## Review set

Six screens, each with a distinct purpose; no native screenshots:

1. [Before: ungrouped current source](00-before.png).
2. [Menu actions, recommended](01-menu.png): three sections, newest first, one row menu.
3. [Inline actions](02-inline.png): the same fixture and grouping, separate Pin/Archive targets exposed by keyboard focus.
4. [Search inside a collapsed project](03-search.png): temporary expansion and matching/total count.
5. [Archive and Restore](04-archive.png): removed from active groups, selected archived content read-only, pin retained.
6. [Narrow List navigation](05-narrow-light.png): separate list page, Back, wrapped long title; light theme at 20 px.

Unchanged geometry/assets: current workspace chrome, toolbar, title, reading area, Details and composer; bundled Lucide SVG bytes and the existing palette/type scale. Intentional differences are CS1–CS6's sidebar headings, order, counts, row controls, search and wrapping. The frozen reference is untouched. The preview uses existing arrow-down rotated on collapse rather than adding a new icon asset.

## Verification

Environment: local macOS, Playwright Chromium headless shell, CSS-pixel viewports 1100×850 and 500×850, 16/20 px, light/dark. Production bundle base: `51c19f9e95f0b5aaf494c732abc24ad75b2f110c`; the candidate commit contains the fixture, specification and instrument together.

`node tests/conversations-sidebar-design.browser.cjs`:

- Before: current-source sidebar has zero groups and zero row-action menus (CS1/CS4 gaps). This is source-backed browser evidence, not an installed-app reproduction.
- After, proposal only: unique membership; Pinned/project/Other order; descending row timestamps; Pin/Unpin; collapse across reopen; search revealing collapsed matches and restoring collapse; task-event/project reorder preserving selection; local Archive/Restore retaining pin; keyboard row-menu and inline actions; 16 visible-list geometry cells; zero page errors.
- The instrument initially exposed overlapping labelled action targets and missing disclosure assets; it now uses normal labelled buttons and the existing bundled arrow asset. Visual inspection also found an archived composer and a hidden-list geometry check; the corrected instrument requires the archived composer to be absent and the tested list to be visible.

Canonical gate after rebase onto `c761cc2ed608b26638bc295dc5f3e5a6690ffb68`: `python3 scripts/check.py --base c761cc2ed608b26638bc295dc5f3e5a6690ffb68` — **sandbox-blocked**, not PASS, after 109.157 s on the local warm-cache Mac. Whitespace, selector/rule checks and web build passed; Rust reported 574 passed, 18 failed, 33 ignored. Failures cover host installation/policy/capacity, Stop/restart, recovery and resource assertions; explicit refusals include `a worker cannot install the host service`, `a worker cannot change host policy` and `a worker cannot restart the host service or request Stop`. The process retains its worker attempt identity; no permission marker was removed and no real host setting was changed. Later canonical steps did not run. The affected browser instrument was run separately after the failure and passed. CI remains required; this design checkpoint does not publish or merge the feature (§7/R12).

| Requirement / boundary | Status | Evidence or limitation |
|---|---|---|
| A1 usability/specification/variants | Pass | Review, CS1–CS6 and two action variants |
| A2–A6 proposal interactions | Pass in fixture | Browser check above; no product implementation claim |
| Whole affected workspace visual review | Pass | All six final images inspected at actual size; title/actions/groups readable, no overlapping popup targets |
| Production timestamp projection and durable local metadata | Not tested | Stage 2 must connect all three source types to authoritative activity and existing native receipts |
| Production search, archive safety, drafts, anchors, deletion | Not tested | Existing controls/guards must be retained and rerun in stage 2 |
| Owner visual acceptance | Pending | Choose Menu or Inline on #1808 |
| A7 installed Mac/Windows | Not tested | Owner answer moves native proof to the §7.8 release batch |

R12 separates this fixture evidence from installed behavior. R13 leaves active decisions unchanged until the owner answer. Stage 2 updates taskq.md § Product first, implements the selected design, records failure/pass protection, then follows §7 publication; merge-result CI is still required for completion.
