# On-demand Filter revision

Owner answer 6289920580 requests a collapsed Filter control, selection in its label,
no usage hint, and the task list directly after the header. R13/Product and UI25 now
record this accepted behavior. UI26–UI28 selection semantics and safeguards remain.

The existing browser test first failed with `missing on-demand Filter disclosure`.
It now passes, retaining the single/multi/empty, counter, keyboard, Select all,
Clear all, project-scope, Back, refresh and no-queue-write checks. Added checks cover
closed-by-default, Space/Enter disclosure activation and the All label after clearing.
The unchanged 320/360 px and 14/16/18/20 px width check exposed a rotated disclosure
arrow overflowing by 1–2 px; replacing its rotation fixes it. No assertions weakened.

[Folded](folded.png) · [Open, one state](single.png) ·
[Open, several states](multi.png) · [No selection: All](empty.png).

Images use the actual bundled renderer with invented data, Chromium headless,
360×760, dark 16 px. Visually inspected: selection label, collapsed controls,
visible task list, checkbox states and no hint text. Browser evidence does not
qualify native filter interaction. Owner approval is pending; not deployed.

Reproduce after `npm run build` with the existing Playwright setup:

```sh
FILTER_EVIDENCE=docs/design/evidence/1350/revision-2 node tests/tray-state-filter.browser.cjs
```

## Verification

Tested source: `f2bc48bc5bc724ad86aae063f5f34ddcd5c8a342`.

- `node tests/tray-state-filter.browser.cjs`: PASS after the recorded failures above.
- All `tests/*.test.cjs`: PASS; prior risk protection retained.
- `python3 scripts/check.py --base f8cde9fa`: FAIL, 403 passed, 18 failed,
  34 ignored; 101.354 s. Explicit host/lifecycle worker refusals are sandbox-blocked;
  other assertion causes remain unknown. No permission override or retry.
- Isolated `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest`:
  85 traces and formats PASS; telemetry committed 3 observations; board unknown.
- Signed native README captures refreshed through the cargo runner as in the first
  revision. Project/workspace images remain unchanged; refreshed icon inspected.
  `sh docs/shots/check.sh` and `git diff --check`: PASS.
- R8: 297 lines, 61439 bytes; within 300 lines and 61440 bytes.

R13/Product and UI25 are amended; UI26–UI28 remain unchanged. Native filter
interaction, owner visual approval and merge-result CI remain outstanding (R12).
The evidence-only follow-up commit leaves tested UI sources unchanged.
