# PM runbook

Under [taskq.md R3](../../taskq.md#r3-roles), the queue drives itself. The PM steers, files new tasks and relays the questions that only the owner can answer. No started task waits for the PM.

## Contract detail

- Intake: `taskq add` always writes a held task (§ 1). The hold exists only for the read-back. Read the task back with `taskq inbox N`, then run `taskq start N` in the same turn.
- Dependents: a task that waits on another gets `--deps M` (one `Depends on #M` line) and is started at once. It stays `waiting` and becomes `ready` when #M closes. Never hold a task to wait for another task.
- `taskq status` flags a held task whose body names an open task `#M` without a `Depends on` line: `PM: names open #M without Depends on`. Add the line, then start the task.
- Worker blockers: a worker files its blocker held and parks with `later N --blocker #M`. The park starts a held local #M, so the blocker runs and the parked task resumes when it closes.
- Stages: a worker stage park without `--until` or `--blocker` resumes at the next service pass. A stage boundary never waits for the owner or PM. A worker that needs the owner between stages uses `ask`.
- Questions: workers and reviewers take the recommended option for technical choices and `ask` only for a product, scope or visual choice (§ 7 rule 3). A technical ask that still arrives is answered by the PM with its recommended option (`taskq answer N.K`). Product, scope and visual asks go to the owner with the task link; the PM never answers them.
- Before relaying an ask, read the fresh task and its accepted answers with their original question revisions. Reuse a clear owner decision within its stated scope; relay it with its exact source, never manufacture a new owner answer from a PM summary. A repeated question is not a new product choice. Ask the owner only for a material unresolved choice or conflict, explaining what changed since the earlier answer.
- An explicit owner decision changing acceptance is reconciled before resume: keep each `A<k>` identity, update only the decided criteria, cite the question revision and original answer in the task record, read the body and current card back, then answer that card with `--revision` and a stable `--op-id`. If the body or card changed concurrently, reconcile fresh state before continuing. A note alone does not amend acceptance; an uncertain write requires readback, not another question to the owner.
- Every handoff carries the original user-authored decision evidence, its scope, task/question revision and any later correction. A short answer such as "yes" travels with the exact question it answered. Preserve the owner's preference against repeated confirmations; separate a real new choice from an already authorized step.
- Task decisions and evidence do not create runtime grants, exact-SHA approval, new data recipients or action-time confirmation. Preserve those checks; after a denial, check new relevant user evidence and retry only when permitted. Report a remaining mandatory boundary as that boundary, not as a fresh product decision.
- The PM parks (`later`) or requeues only by an owner decision or to fix a defect, never as a wait.
- Manager mode: `taskq pm` prints the session steps (timer, each tick) and `taskq pm stop` ends them. A session runs them exactly. The tick answers technical asks itself and asks the person only product, scope, visual and money questions.
