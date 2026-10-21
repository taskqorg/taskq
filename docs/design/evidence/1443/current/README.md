# Current delta after #1449

The frozen #1442 baseline was reviewed before main advanced. Rebase brought in
`8669a08ec2435e830301bf7307fb37f477c87e6d`:
[#1449](https://github.com/taskqorg/taskq/pull/1449) implements the owner's #1438
decision, and #1447 changes host-record durability. This task adds evidence only.

R6 links [the current counter contract](../../../task-counters.md): the top strip
keeps all eight state controls, icon-only zeros, explicit unknowns, OR toggles and
an All tasks open total excluding Closed. All tasks clears state/project scope
and groups nonempty projects with their own counters/tasks. Project summaries
still omit zeros. Filters remain view-only.

The local [v2 profile](../../../ui-rules-v2.md) was not updated by #1449 and still
describes the old disclosure, mixed checkbox, old shortcuts and zero omission.
That is a documentation gap under R8/R13; R6 and its linked counter contract win.
D in the [revised owner plan](../owner-plan.md) maps those clauses to the already
approved behavior, without restoring the old UI. The custom owner answer
supersedes the old H/T/S/L/U briefs; this delta retains measured-source identity.

## Current observations

[304 text verdicts](matrix.md) now check all 19 concrete routes/views at both
themes, 16/20 px and 320/360 px, with fine/coarse pointers. Owner-updated A1/A2
replace screenshot-per-cell output. The instrument emits no PNGs; only
[eight distinct-gap images](../screenshots.md) remain for review. All current
cells have a measured header/target/type gap; zero page errors occurred.

[Observations](observations.json) name checkout
`02d3383cc2fb595e3c93c09303aaed1fe0ecf93f`, whose product source is unchanged from
`8669a08ec2435e830301bf7307fb37f477c87e6d` (only this task's evidence was added).
[Source identity](source.json) hashes the checked product source, compiled bundles
and revised text-only instrument. Reproduce with the single shared instrument:

```sh
node docs/design/evidence/1443/capture.cjs .taskq/review-1443/text-check
```

The prior visual review and browser checks of the OR-state summary, All tasks 22
fixture total, project grouping and independent targets remain source-labelled
observations. Passing behavior is retained as text; its screenshot variants were
removed. A table pass would cover only the instrument's measurable checks, not
all 38 rules, installed-platform behavior or owner visual acceptance (R12).

The unchanged header/Back, target-size, repeated identity, spacing/type, contrast,
workspace focus and announcement findings remain applicable after source-diff
review; no upstream repair to those paths was found. The retained gap images and
text measurements show Back/title in content, 20 px bold titles, repeated Alpha
identity and small targets. New zero buttons also need N's effective-width checks.

## Before and after from upstream

- Before, `8bf66914f03560c8c3f979523dce7d6781fe5420`: the shared project-block browser check fails at its obsolete `project name · Review` expectation. The full exact diagnostic is identified in the main report.
- After, `8669a08ec2435e830301bf7307fb37f477c87e6d`: upstream changed that assertion to test the actual filter header/project/pressed state while retaining independent navigation assertions. `node tests/project-blocks.browser.cjs` passes 64 parent/browser/tray comparisons, wrapped right edges, 0/1/12/100 counts, both themes, 16/20 px, 320/360 px, navigation, long content, partial/provider cases.
- Current `node tests/tray-state-filter.browser.cjs` passes K-A2/K-A6/UI25–UI28: toggles, icon-only zero/unknown, OR, All tasks, keyboard, refresh/Back, no writes, eight theme/type/width cells.
- The original `npm.cmd run build` and 80 changed-route checks completed; their screenshot variants were removed. The revised instrument checks all 304 cells, reports 304 measured gaps and zero page errors, and writes no PNGs. This verifies the review-output correction, not a UI fix. Native evidence and complete keyboard/contrast/state coverage remain unknown.
- The full local canonical gate failed on the original exact source; [its 21 failures](../checks.md) remain recorded. It was not rerun after these upstream merges; no full-current-source gate or merge-result CI pass is claimed. The affected browser checks were rerun because their source changed; unchanged workspace/answer results remain explicitly tied to the original source.

The before/after repair belongs to upstream #1449. This task neither fixes that
assertion nor changes a production rule. The revised owner plan preserves the
new R6 behavior and no longer includes repairing an assertion already fixed.
