# TaskQ operator UI acceptance

Apply [shared D1–D14](https://github.com/taskqorg/specq/blob/main/design/operator-ui.md).
Apply [shared implementation fidelity F1–F6](https://github.com/taskqorg/specq/blob/main/design/implementation-fidelity.md) and [native acceptance](ui-acceptance.md).
This file contains TaskQ-specific checks, not a second definition of the shared principles.
[Contract](../../taskq.md#product) · [Menu](menu.md) · [Square summary](icon-summary.md).

## Product boundaries

- Menu behavior, renderer, registry, AI, navigation and maintenance belong to M1–M12 in the menu specification.
- Square geometry, font calibration and platform qualification belong to I1–I5 in the icon specification.
- Menu counters use nonzero symbol capsules; zero leaves no slot. The summary icon retains zero in its fixed positions (M1/M3).
- Task rows keep issue numbers and host names intact; long titles wrap under metadata from the block's left edge (M3).
- Specifications are design targets. Runtime follows taskq.md until implementation and verification establish otherwise.

## Acceptance

Apply the approved [app UI v2 profile](ui-rules-v2.md) and its pinned shared V checks.
Keep the historical acceptance identifiers below.

Apply the approved [UI01–UI28 menu checklist](ui-rules.md#checklist-28-rules) alongside the existing D acceptance IDs. The [baseline review](ui-rules.md#observed-menu-findings) is browser evidence, not native acceptance.

| ID | TaskQ evidence |
|---|---|
| D-A1 | Status, issue number and title start in one flow; continuation uses the full block width. |
| D-A2 | The task body opens its issue. Separate session/PR links do not also open the issue. Project identity stays unambiguous. |
| D-A3 | Menu font changes apply throughout and persist; icon sizing remains independent. |
| D-A4 | At 320/360 px, long titles produce no collisions or horizontal scrolling. |
| D-A5 | Both themes at 100/200% scaling preserve the grid and visible-edge alignment. |
| D-A6 | Meaning survives without color; keyboard, screen reader and touch targets work. |
| D-A7 | The menu contains no empty groups or redundant content. |
| D-A8 | Compact counters hide 0; symbols with 1/12/100 fit; unknown stays visible. Wrap as a group under K rules, never stretch across empty width. |
| D-A9 | Revision preserves approved palette, icon assets and row geometry; intentional changes trace to current owner corrections. Task dividers stay quiet; raised panels remain distinct in both themes. |
| D-A10 | All text/file entry uses the shared composer; switching, validation and archive/Undo preserve independent drafts and never send. Unsupported capabilities remain explicit. |
| D-A11 | Named-host/runtime counters and resource meters match their sources; stale/unknown remains distinct. Completed tasks expose outcomes and history independently of activity totals. |
| D-A12 | Empty task/thread search occupies one labelled icon in its existing context row; opening focuses it. A nonempty query stays visible, including zero matches. Clear restores the list; empty blur/Escape restores the icon. Applied scope and focus never disappear during entry. |
| D-A13 | Held batch admission uses a distinct action beside the existing counters, with visible Held count and exact scope in its accessible name/tooltip. It adds no dedicated row, never repurposes a filter chip and keeps source task keys/revisions. |
| D-A14 | Review Menu, Conversations, task context/activity, Views and every Settings route under shared D4/D5/D11/D13: no row for an empty secondary control, no unnecessary disclosure on a primary path, no hidden active filter/error/decision and no inferred-frequency rearrangement. Record rows/clicks before and after; preserve all capabilities, readable type and 32/44px targets. |
| D-A15 | Project headings keep board, project name and folder in one left-aligned row. Each target acts directly: exact registered board, existing project overview, or registered local root. Back preserves list context; missing destinations have an explicit unavailable state. No inferred paths or hidden task admission. |

General improvements follow [shared-rule improvement](https://github.com/taskqorg/specq/blob/main/engineering/spec-first.md#improve-shared-rules). TaskQ behavior belongs in taskq.md and its linked specifications.
Preserve these acceptance IDs when refining product checks.
