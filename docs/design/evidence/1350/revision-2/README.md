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
qualify native filter interaction. The owner [approved this revision](https://github.com/taskqorg/taskq/issues/1350#issuecomment-6290307150); not deployed.

Reproduce after `npm run build` with the existing Playwright setup:

```sh
FILTER_EVIDENCE=docs/design/evidence/1350/revision-2 node tests/tray-state-filter.browser.cjs
```

## Previous verification

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
interaction and merge-result CI remain outstanding (R12).
The evidence-only follow-up commit leaves tested UI sources unchanged.

## Approved candidate verification

Rebased onto `68bb242b`; tested source `e42ad010c809032b0e9752f4b21b73bb0bc9af81`.
The UI sources, browser instrument and approved images are identical to the
approved revision. The rebase retains upstream telemetry changes (R1/R13).

- `python3 scripts/check.py --base 68bb242b`: FAIL, 409 passed, 18 failed,
  34 ignored; 66.752 s of the 900 s budget. Nine failures explicitly refuse worker
  host installation, policy or lifecycle operations: sandbox-blocked. The other
  nine assertion failures remain unknown. No worker guards were removed.
- `node tests/tray-state-filter.browser.cjs`: PASS; the existing regression
  protection for selection, counters, keyboard, scope, refresh and Back remains.
- All `tests/*.test.cjs`: PASS, run separately because the gate stops at Rust failure.
- Isolated `TASKQ_SELFTEST_OFFLINE=1 cargo +stable run --locked -- selftest`:
  85 traces and formats PASS; 3 telemetry observations; board unknown.
- `sh docs/shots/check.sh` and `git diff --check`: PASS. The existing native
  captures remain applicable because their UI source blobs have not changed.
- R8: 297 lines, 61411 bytes; within 300 lines and 61440 bytes.

Owner visual approval is recorded. Native filter interaction remains unverified;
merge-result CI (`merge_group`) is required before publication. The documentation
follow-up changes no tested implementation or test code. Not merged or deployed.

## Conflict repair after the running-agents merge

Rebased onto `8931e00764293156939bd89c260e5d48adda704a`; tested implementation
`976611fc`. Before: GitHub reported PR #1368 CONFLICTING and rebase stopped on
`docs/shots/manifest.json` and its icon capture. After: rebase completed, preserving
the approved filter and upstream running-agent UI. Refreshed native README shots
with `sh docs/shots/update.sh`; project/workspace remain unchanged, icon inspected.

- `node tests/tray-state-filter.browser.cjs`: PASS; all four output images are
  byte-identical to the owner-approved revision. Folded and multi inspected.
- All `tests/*.test.cjs`: PASS, including navigation and running-agent checks.
- `python3 scripts/check.py --base 8931e00764293156939bd89c260e5d48adda704a`:
  FAIL, 409 passed, 18 failed, 34 ignored; 91.617 s. Explicit worker host/lifecycle
  refusals are sandbox-blocked; remaining assertion causes are unknown. No guards
  or assertions changed. An initial invocation used the stale local-main base
  and stopped at whitespace in upstream evidence logs; those files were not edited.
- Isolated offline selftest: 85 traces and formats PASS; 3 telemetry observations;
  board unknown. `sh docs/shots/check.sh` and `git diff --check`: PASS.
- R8: 297 lines, 61401 bytes. R13/Product and UI25 changes retain owner approval.

No new behavior or test changes in this repair. Native filter interaction remains
unverified. Merge-result CI is required before publication; not merged or deployed.
