# State filter acceptance evidence

Implements the approved UI25–UI28 targets from #1348 under R13. Counter clicks select
one state; explicit native HTML checkboxes add/remove states in one action. This follows
[W3C's checkbox pattern](https://www.w3.org/WAI/ARIA/apg/patterns/checkbox/): independent
options, Space activation and mixed select-all. Modifier-click is not required.

State selections use OR within the current project scope. Clear all leaves no restriction;
Reset selects all. Selected counters have an outline and `aria-pressed`. The retained
WebView state survives hide/reopen and snapshot refresh; this does not persist across an
application restart. Filters send no queue, focus or board commands.

## Before and after

`node tests/tray-state-filter.browser.cjs` failed before implementation:
`UI25: missing explicit state checkboxes (0 !== 1)`. The same scenario passes after:
single-state switching, adding/removing a state, mixed/all/empty selection, keyboard
focus, project scope, refresh, Back, unknown Closed and no mutating bridge messages.
Navigation assertions remain; the scalar filter expectation now checks a one-item array.
The instrument also checks 320/360 px, 14/16/18/20 px and both themes for filter overflow
and 32 px option hit targets. It is browser evidence, not native platform qualification.

[Before](before/before-or-single.png) · [Single](after/single.png) ·
[Multiple](after/multi.png) · [Empty selection](after/empty.png).

All three after images were visually inspected: selected outlines, checkboxes, Clear all,
result counts and full task titles are visible. Images use invented data in the actual
bundled tray renderer, Chromium headless at 360×760, dark 16 px. The fixed viewport does
not qualify native content-fit sizing. No native click/reopen observation is claimed.
Owner visual approval is required before merge (A4); these images do not constitute it.

Reproduce after `npm run build`, with the existing Playwright setup:

```sh
FILTER_EVIDENCE=docs/design/evidence/1350/after node tests/tray-state-filter.browser.cjs
```
