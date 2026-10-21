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
Draft L in the main report includes mapping those clauses to the already approved
behavior, not asking for a new filter policy or restoring the old UI.

## Current observations

[80 captures](matrix.md) recheck every route changed by the tray delta: Home,
Projects, Project, filtered list and All tasks, in both themes at 16/20 px and
320/360 px, with fine/coarse pointers. They use the same instrument and invented
fixture as the baseline, adapted only for the new All tasks control. Main cells
require matching consecutive rasters and have zero page errors. The unchanged
routes retain their explicitly source-pinned baseline evidence, not a false claim
of fresh captures on this revision. Native acceptance is still unknown (R12).

[Observations](observations.json) pin the product source and image hashes;
[source identity](source.json) pins the current source, compiled bundles and
adapted instrument. Reproduce after building with the existing Playwright setup:

```sh
node docs/design/evidence/1443/current/capture.cjs
```

Visually inspected current filtered dark/20/320/fine, All tasks
light/20/320/fine and Projects dark/20/320/coarse at actual size. These confirm
the OR-state summary, All tasks 22 fixture total, per-project sections and
independent target groups. This is browser evidence, not installed platform proof.

The unchanged header/Back, target-size, repeated identity, spacing/type, contrast,
workspace focus and announcement findings remain applicable after source-diff
review; no upstream repair to those paths was found. A current filtered screenshot
still shows Back/title in content, 20 px bold title, repeated Alpha identity and
small targets. New zero buttons also need T's effective-width checks.

## Before and after from upstream

- Before, `8bf66914f03560c8c3f979523dce7d6781fe5420`: the shared project-block browser check fails at its obsolete `project name · Review` expectation. The full exact diagnostic is identified in the main report.
- After, `8669a08ec2435e830301bf7307fb37f477c87e6d`: upstream changed that assertion to test the actual filter header/project/pressed state while retaining independent navigation assertions. `node tests/project-blocks.browser.cjs` passes 64 parent/browser/tray comparisons, wrapped right edges, 0/1/12/100 counts, both themes, 16/20 px, 320/360 px, navigation, long content, partial/provider cases.
- Current `node tests/tray-state-filter.browser.cjs` passes K-A2/K-A6/UI25–UI28: toggles, icon-only zero/unknown, OR, All tasks, keyboard, refresh/Back, no writes, eight theme/type/width cells.
- `npm.cmd run build` and the 80-cell capture pass. Native evidence and complete keyboard/contrast/state coverage remain unknown.
- The full local canonical gate failed on the original exact source; [its 21 failures](../checks.md) remain recorded. It was not rerun after these upstream merges; no full-current-source gate or merge-result CI pass is claimed. The affected browser checks were rerun because their source changed; unchanged workspace/answer results remain explicitly tied to the original source.

The before/after repair belongs to upstream #1449. This task neither fixes that
assertion nor changes a production rule. The final proposed briefs preserve the
new R6 behavior and no longer include repairing an assertion already fixed.
