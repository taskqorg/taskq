# Review brief: TaskQ interface redesign (#1573 package)

You are reviewing finished work with the owner in a live conversation. The owner will ask questions and give remarks. Answer from evidence (code, contracts, rendered screens), not from task close texts.

## What to review

The owner accepted a design package on 2026-10-22 and the PM split it into two delivery tasks. Both are closed and merged. The owner now has many remarks and doubts that everything was done right.

- Parent plan: https://github.com/taskqorg/taskq/issues/1573
  - Package ZIP and hashes are in its comments (archive SHA256 `d9f39f3f…`, frozen prototype SHA256 `9c250e85…`). Read `taskq/docs/design/implementation-brief.md` inside it first.
  - Owner decisions O1–O3 are in the comment "Owner decisions — planning continuation":
    - O1: rows show only the current stage and its elapsed time.
    - O2: fixed state order, then priority, then issue number.
    - O3: working views apply at once; only Save updates a preset.
- #1578 Menu, dashboard and settings: https://github.com/taskqorg/taskq/issues/1578
  - Delivered by PR #1590, merged as `635f9b16`.
  - Close says: A1–A8 ok, macOS only.
  - Open on close: Windows geometry and DPI, physical gestures and native select keyboard are unverified.
- #1580 Conversations and attachments: https://github.com/taskqorg/taskq/issues/1580
  - Delivered by PR #1612, merged as `9661f2be`.
  - **Important:** the owner answered option 2 on its ask. The dialogue stays native-only. The workspace has drafts, files, Recent/Archive/History, Undo/Restore, detach and "Open session", but no in-workspace reply chain.
  - Open on close: native Read handoff, IME and accessibility, and Windows DPI. PDF is unsupported. The 20 px narrow header wraps "Conversations" mid-word.
- Related closed work in the same iteration:
  - #1591: freshness rule.
  - #1592: one display model for CLI, tray and notifications.
  - #1598: one icon renderer.
  - #1615: PM request composer.
  - #1599: dead code removal. It deleted some unrun previews and prototype tests.

## Questions to answer for the owner

1. Per acceptance line of #1578 and #1580: is it really done in the current build? Answer done, partial or missing, with file:line or screen evidence. Do not repeat the worker's "ok".
2. Real data versus form. Which real data the menu and workspace show (tasks, stages, timing, agents, resources, history, telemetry, conversations) has no good display form? Examples: unreadable, truncated, too dense, wrong grouping, no place at all. Propose a form for each.
3. Bugs: anything broken, inconsistent between tray and workspace, or different from the contracts.
4. Sections that need more work: compare against the package and the contracts. List what is thin or placeholder.
5. Conversations: is the section complete for what was decided (answer 2)? Should the decision be revisited (a native-backed reply bridge was option 1)? Say what the owner gets and does not get today.

## Where things are

- Repo: taskqorg/taskq, branch `main`.
  - Contract: `taskq.md`, mainly § 4 for the tray and Conversations.
  - Design docs, all under `docs/design/`:
    - `implementation-brief.md`
    - `menu.md`, `menu-product.md`
    - `task-counters.md`, `task-controls.md`, `custom-icon.md`
    - `ui-acceptance.md` and `ui-acceptance-report.md`
    - `ai-workspace.md` (C1–C9, C-A1–C-A10)
    - `pm-requests.md`
    - `exploration/control-panels-review.md` (CW1–CW8)
- UI source:
  - `src/*.js`, `src/*.css` (Preact).
  - `src/conversations.rs`, `src/conversations.js`.
  - `scripts/build-web.cjs`.
- Rendered screens:
  - Run `taskq tray --snapshot DIR [--route R,..]` with the Mac dev binary `~/.local/share/taskq/taskq`. It captures icons and popovers in both themes from live data.
  - Owner rule: at most 10 screens per review set. List passing cells as text, without PNGs.
- The live tray on this Mac runs the dev build of main. The owner can show you the real menu and workspace.

## Rules for this session

- Review only. Change no code, open no PR, start or close no task, and change no service, grant or setting.
- Give findings as a list:
  - one line per finding;
  - severity first;
  - evidence link next;
  - a proposed fix.
- At the end, group the findings into a few coherent fix tasks (one user outcome each, not one per component). The PM files them on the taskq board after the owner agrees.
- Talk to the owner in short Russian; write findings in English.
- Never print tokens or other secret values.
