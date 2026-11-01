# Task counters

Approved: aggregate above Projects; all queue states available; Held replaces Closed (owner, #1476). Human = Ask: tasks waiting for a human answer (owner confirmed).

## Semantics

- Home: one compact aggregate strip immediately above Projects. Scope: all registered observed boards, not just pinned/recent or admitting projects. Click opens the corresponding task list.
- Every visible project row (pinned, recent and All projects) includes its own nonzero counters beside its name; aggregate counters never replace them. A known all-zero project shows its name only, without an empty strip or `No tasks` caption.
- Project: same order and symbols, only that board/project. Deduplicate by provider/server/repository/issue across registrations/hosts; conflicting or stale observations mark incomplete.
- Order: Doing, Review, Ask, Failed, Ready, Blocked, Later, Held. No Work total alongside Doing/Review; that would count twice. Exception: All tasks shows a separate deduplicated open-task total across all observed projects, excluding closed tasks (owner decision #1438); failed/stale projects are excluded under taskq.md section 4; conflicting task observations show `?`.
- Project summaries hide zero capsules. The tray's top task-list strip shows every state in the fixed order: zero shows its icon without a number; unknown ([never healthy](https://github.com/taskqorg/specq/blob/main/engineering/verification.md#unknown-is-never-healthy)) shows `?`. Empty accurate state says `No tasks`. Failed/stale sources each show one plain name-and-cause line (taskq.md section 4); conflicts remain explicit unknowns.
- The top strip is the only state filter: clicking toggles a state, several selected states combine with OR, and no selection removes the state restriction. Selected capsules have a visible outline and `aria-pressed`. No Filter checkbox group remains. All tasks sits in the same wrapping row, shows the selected project/search total before state filtering and clears only state restrictions (O2/MP11); project scope is cleared explicitly in the overlay editor; incomplete projects remain labelled. Filters never write queue or admission focus.
- `Ask` means a human answer is needed. It is not human execution. `assignee-only` is an admission constraint, not a human-owned task state. Add a separate Human execution counter only after a state/data contract exists.
- Held: open drafts in `held` (taskq.md § 1), counted from snapshot rows like other states. The top strip joins the Held filter/count and Play-only Start as sibling buttons with a divider and independent accessible names. The group wraps as one unit, with no dedicated row or distant duplicate action. The filter keeps OR/pressed behavior. Start resolves deduplicated Held targets in the current project/search/view scope before state filters, including pinned drafts; Doing-only cannot hide them. Its tooltip/name specifies count, projects, query and view. Zero or unknown scope disables Start. Review freezes canonical task keys and revisions; confirmation rejects changed scope/targets, submits once, and retains existing per-item pending, failure/unknown, same-operation retry and ready/waiting readback. Search, view and focus remain unchanged. Per-task Start remains available.
- No Closed counter. Closed tasks appear only in the Completed view ([MP14](menu-product.md#dashboard-decisions-1573)) and in pinned references with their sourced outcome ([P-W1](menu-product.md#windows-and-task-pins)); the state strip never lists them. Tray stays token-free and reads status only.
- Breakdowns by task type are filters, not extra additive lifecycle counts. AI failures and service incidents remain separate.
- Square tray summary stays Work = Doing + Review, Ask, Ready, cadence. These expanded counters are menu-only.

## Visual

- Compact wrapping capsules with stable order; symbol + number, accessible state name and tooltip. Align every strip to the right edge of its content region, including the aggregate and each wrapped line. Keep the existing compact gaps; do not distribute capsules across empty width.
- Doing green, Ask amber, Failed red under [palette A](icon-summary.md#i4-marks-and-palette). Review/Ready/Blocked/Later neutral; shape carries identity.
- Held last: neutral, like Ready/Blocked/Later; shape carries identity.
- At large type, wrap counters together beneath the project name. Never shrink numbers or squeeze project titles into a narrow column.
- Counter clicks are separate targets from project navigation. One project name row, then its compact strip if needed; no nested controls.

## Acceptance

- K-A1: same task registered twice counts once; Doing and Review stay disjoint.
- K-A2: the top task-list strip shows all eight states in order, including icon-only zeros and explicit unknown; project summaries retain nonzero counters.
- K-A3: Held counts open `held` rows before state filtering. Its joined scoped Play-only Start reviews frozen keys/revisions, rejects changed targets, sends once and reads back `ready`/`waiting` or each failure/unknown. Zero/unknown cannot admit all; per-task Start remains. No Closed capsule or totals.
- K-A4: aggregate includes projects outside shortlist; scope and partial sources inspectable.
- K-A5: 320 px and 20 px type wrap without horizontal overflow; Held readable in both themes.
- K-A6: Held filter and Play-only Start are separate sibling buttons with a divider, nonoverlapping keyboard/touch targets and one wrapping group, without an extra row; Start names count/project/search/view scope and preserves search/view/focus. Counter clicks toggle OR selection with visible and accessible pressed state; All tasks shows the scoped deduplicated open total, Held included, and removes state filtering while retaining projects/search (O2/MP11); Back/refresh retain selection; no checkbox group or queue/focus write.

- K-A7: pinned/recent/All projects preserve per-project nonzero counters; known all-zero rows show only project identity; each counter opens that project and state, without triggering the project-name action.

- K-A8: aggregate, project and filtered-view strips share right alignment; every wrapped line ends at the strip right edge at 16/20 px and 320/360 px without changing state order.

The Held count appears once on its filter; the sibling action renders only the bundled Play glyph. Its full accessible name and tooltip retain exact count, project/search/view scope and the frozen-target review (K-A3/K-A6).
