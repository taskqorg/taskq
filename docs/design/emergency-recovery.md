# Emergency stop and recovery

Required capability approved. E1–E5 describe the mechanism; [taskq.md](../../taskq.md) § 3 and § 4 implement the host hold, liveness reconciliation and fenced recovery (`taskq service hold|release|recover`). Drain and Full stop are approved Mac modes. Selective recovery retains the hold for unselected work. Product entry: MP6. Pause only blocks new admission; service restart is not recovery.

## E1 Scope and review

- Fixed stop icon is reachable on every menu page; accessible name Emergency stop. Distinct shape from pause; red only for this consequential action/status.
- Review names this host, projects, worker/reviewer/operator attempts and in-flight service operations. Default scope is all local managed work; remote hosts are explicitly unaffected.
- Confirm once. Stop new launches and state-changing operator applications immediately; keep status, diagnostics and stop/recovery control available. Report operations already past a cancellable boundary as uncertain until reconciled.
- Do not kill by executable name. Address recorded process/session identity and owned descendants; unrelated terminals and agents remain untouched.

## E2 Durable stop

- R11 identity is the recorded native session ID, never a name match. A completed Claude background turn is stopped after native completion readback, and a CLI child after its log shows the turn ended (§4 step 1); retain each conversation for the next turn. Uncommitted work is never lost: only `cleanup --force` removes it, after saving it to `<root>/.taskq/trash/taskq-<N>.patch`.

- Drain holds new admission and lets current work finish. Full stop immediately requests cancellation of all owned worker, reviewer and operator executions in the reviewed local scope, including while the board is offline. Both retain a durable host hold; Pause remains separate.
- The existing host lifecycle request binds the named host generation, registered roots and command identity. Full stop requires its advertised capability. Cancellation is attempted through retained native handles, never inferred from status age. The daemon remains available while termination or board reconciliation is unresolved.

- Persist the Stop request identity and reviewed roots in the existing host hold before terminating work. Per-project recovery follows E3. It survives service restart, logout and app reopening; ordinary Play cannot clear it.
- R1 host-hold exception: the interlock stores admission safety scope and operation identity, not a second task queue. Board remains task-state authority.
- Full stop uses each existing native cancellation adapter. Unsupported or failed cancellation stays unknown; no generic executable-name kill or extra timeout is added. The review warns that unsaved editor buffers may be lost.
- State per attempt: requested → stopping → stopped / failed / unknown. Only observed process/session termination proves stopped. Unreachable runtime stays unknown and blocks relaunch.
- Preserve worktrees, diffs, untracked files, logs and session references. No cleanup, reset or stash as an implicit side effect. Disk writes survive; memory-only edits and interrupted external effects are not guaranteed recoverable.
- Offline board: Full stop may cancel owned execution before task retirement (R11); keep the interlock. Reconcile task events when reachable; do not report board retirement or resume early.
- Multi-host safety requires every service to honor unresolved executions; lease expiry never permits another host to claim. Incompatible old services must be drained/stopped before cutover. If board communication is unavailable, local stop cannot promise a global stop. Show that limit explicitly.

## E3 Recovery

1. Keep the host held. Read current board, service, process/session identities, leases, workspace status and last operation receipts. Evidence: [specq verification](https://github.com/taskqorg/specq/blob/main/engineering/verification.md).
2. Show one row per interrupted attempt: task, runtime/host, confirmed stopped or unknown, preserved work, last known action and blocker. Unknown remains unselected and disabled for relaunch.
3. Retire/fence the previous attempt through the board model. Reject late output; prove no live old process before creating a new attempt. Do not replay external operations with uncertain outcomes.
4. Offer Inspect, Keep held, or Restart from preserved work for eligible rows. Default is Keep held. Same-session continuation is only available when the adapter proves it can bind the new fenced attempt safely; otherwise use a new session with preserved artifacts.
5. Review selected tasks, current permissions/limits/focus and intended admission state. Preserve the saved scope; explain tasks no longer eligible. Resume only named projects and explicitly requeued tasks. Previously paused projects stay paused.
6. Clear only reviewed project entries after checked transitions/readback; other project entries and remaining interrupted tasks stay held. Display partial recovery as Mixed, never all-running. Repeated confirmation/recovery requests use one operation identity and cannot duplicate launches.

Recovery is not rollback. Successful remote writes remain successful; uncertain writes need inspection before retry.

### Selected recovery protocol

- Mac owner CLI: `service recover --json` reads a fresh project review; `service recover --request JSON --json` applies its selected rows. The tray calls this CLI, never the board directly. No worker may submit recovery.
- Each selected row names Stop identity, board identity, task, execution and board revision, plus nonempty owner-reviewed reconciliation evidence. The owner confirms that the evidence establishes the outcome of interrupted effects; unknown effects remain blocked. Evidence cannot substitute for native release or input acceptance.
- Re-read the board and workspace before each write. Refuse changed revision, execution, project registration, paused focus, missing worktree, unreadable Git state, unresolved execution/input, publication in progress or unsupported task state. Dirty and untracked files are preserved, never cleaned, stashed or reset.
- Use one deterministic `requeue` operation per Stop/task/execution/revision (an already-ready task first receives a fenced `later` with its own stable suffix), with the evidence in its public text. A lost response reconciles the same operation read-only before permitting admission; reopening the UI exposes saved pending selections. A pending task refuses a different recovery request. Never requeue again after an uncertain write; a different payload for the same operation is refused.
- The existing hold retains at most 4096 reviewed operations and selected admission scope, not task truth; requests contain at most eight rows and 16 KiB. Admit selected tasks through normal capacity, permissions, focus and execution fences. Unselected tasks and operators remain held. Full stop cancellation excludes only tasks with a confirmed recovery receipt. The UI calls this Mixed, never all-running; no automatic whole-host release occurs. After the original Stop receipt proves host exit, confirmation may start existing host supervision once through the canonical installer; unknown startup is reconciled from its installation receipt, never resubmitted. A new owner-confirmed Stop revokes selected admission and retains any pre-existing ordinary hold.
- Existing Stop holds without per-task capture list current open tasks whose last execution belongs to this host. The review identifies these as local recovery candidates, not proof that Stop interrupted each one. Closed tasks are never reopened. A missing execution or unsupported historical evidence cannot become eligible.


## E4 Contract and compatibility work

- R11 defines explicit emergency termination; R1 retains its existing host-hold exception. Stop does not invent a task state or change §2 folding. Selective recovery remains gated by E3.
- Check worker, reviewer and operator paths, including codex-app handoffs. Termination support differs by runtime; unsupported control must remain visible.
- Stop submission requires the owner context and exact fresh host generation/roots; Full stop requires full_stop_protocol 1. Persist the hold atomically and pin the existing launcher rollback floor to this capability-bearing version before cancellation. Ordinary hold/release refuses to overwrite or clear a reviewed Stop. Older/manual binaries are unsupported; never downgrade while the hold remains.
- Choose board event/projection representation without adding an invented task state in the prototype. Emergency held is an execution condition; task counts retain their defined lifecycle semantics.

## E5 Acceptance

- E-A1: pause leaves current work running; emergency stops only owned targeted work.
- E-A2: crash/restart during stopping keeps admission held; duplicate commands do not relaunch.
- E-A3: partial termination and offline board remain explicit; no global-stop claim.
- E-A4: dirty worktree/untracked files preserved; no promised recovery of unsaved buffers.
- E-A5: late old-attempt output ignored; new attempt blocked while old liveness is unknown.
- E-A6: review precedes recovery; only selected eligible tasks resume under current limits/permissions.
- E-A7: previously paused projects and unselected tasks remain held; partial recovery is inspectable.
- E-A8: uncertain external effect is reconciled before retry; no duplicate publication/application.
- E-A9: non-cooperative runtime, old binary, app-session handoff and remote lease expiry have tested limits.

## Contract detail

Moved verbatim from `taskq.md` (#1179) to keep the contract under its R8 size bound. `taskq.md` keeps the decision and links here; this text keeps the same authority (R8, R13).

- Pause: `service pause` (and the tray's) saves the focus to `focus.paused` and writes `off`; resume restores it. Without a saved focus resume fails and names the labels of open tasks: never `all` by default. Emergency ([E1–E5](emergency-recovery.md)): under the § 3 hold the loop still reaps, stops replaced children and beats. A failed liveness read in step 1 keeps the child: `status.json` `kids` `live` `unknown` with its `reason` (else `running`), never `gone`; `hold` names the reason. `service recover` (only under a hold) lists each `doing`/`review` attempt of this host with its liveness from a `status.json` at most 60 s old (else all unknown); a child read running or absent gets the op `gone attempt <id>` with `--revision` (the task's) and `--op-id recover.<N>.<id>`; unknown gets none and blocks relaunch. `--apply` writes the ops through the § 5 dispatcher, one receipt each; step 2 stops fenced children; `gone` never confirms termination. Removing the admission hold still waits for every prior execution release before admitting by focus. Drain/Full stop retain this hold; their selected recovery uses E3 instead of bulk `--apply`.
