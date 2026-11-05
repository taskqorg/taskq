# Repeated PM questions: evidence and remaining design

The owner requested a process fix, not another confirmation: “ты должен проверить как сделать, чтобы очередь не задавала вопросов, которые не нужны”. The preceding complaint was “да - но как сделать чтобы ты запомнил мой ответ - изза тебя очередь стоит”. This report separates preserved answer context from the still-needed reconciliation of an answer with task acceptance.

## Confirmed failure

In both examples, the next agent saw the accepted decision and cited it. The repeated question came from a second source of authority: unchanged Acceptance contradicted the answer. More memory alone does not fix that mechanism. These histories establish unnecessary repeated scope questions; they do not establish that any external approval denial was erroneous.

### TaskQ #2359: one A3 decision asked three times

| Event | Board evidence |
|---|---|
| Worker asks to keep released-service proof in DashQ #255 because TaskQ's completion contract excludes per-task live installation proof. | [Ask 6635328830](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6635328830) |
| Option 1 is accepted. | [Answer 6635428850](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6635428850) |
| Reviewer cites that answer but asks again because A3 still requires released-service proof here. | [Ask 6636976740](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6636976740) |
| The same scope is accepted again. | [Answer 6636988580](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6636988580) |
| Following a separate collection-evidence requeue, a new worker cites both answers and asks again because binding A3 remains unchanged. | [Requeue 6637087547](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6637087547), [ask 6637161218](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6637161218) |
| A3 is aligned, the exact existing candidate and limitations are handed off, and the answer names the open question revision. | [Handoff 6638764876](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6638764876), [answer 6638772063](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6638772063) |
| Review accepts native evidence here, with released comparison still assigned to DashQ #255; normal publication closes the task. | [Publish 6638930679](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6638930679), [close 6639078181](https://github.com/taskqorg/taskq/issues/2359#issuecomment-6639078181) |

### TaskQ #2311: a waiver answered, then asked again

| Event | Board evidence |
|---|---|
| Reviewer asks whether to waive historical CPU-gate-held minutes that neither source recorded, while retaining a separate documentation fix. | [Ask 6635534840](https://github.com/taskqorg/taskq/issues/2311#issuecomment-6635534840) |
| Option 1 accepts the waiver and documentation fix. | [Answer 6635752888](https://github.com/taskqorg/taskq/issues/2311#issuecomment-6635752888) |
| Reviewer requeues the documentation defect and explicitly preserves the waiver. | [Requeue 6635810318](https://github.com/taskqorg/taskq/issues/2311#issuecomment-6635810318) |
| New worker cites the answer but asks again because A1/A4 still require the waived metric. | [Ask 6635858892](https://github.com/taskqorg/taskq/issues/2311#issuecomment-6635858892) |
| Option 1 is accepted again. Current A1/A4 explicitly retain the metric as unknown; the task is completed. | [Answer 6636240228](https://github.com/taskqorg/taskq/issues/2311#issuecomment-6636240228), [current task](https://github.com/taskqorg/taskq/issues/2311) |

## Implementation slice: preserve decision context

The accompanying `core::task_context` change carries an accepted answer together with the original question and its board revision into the existing shared task context. A short answer or numbered choice must remain interpretable after a new worker or reviewer starts. Reuse the existing projection and immutable accepted entries; edited, stale or ignored entries must not acquire authority. Keep ordinary owner notes and their ordering.

This is context fidelity, not automatic acceptance editing. It does not change task lifecycle, declare an old question resolved against changed scope, or prove that a native runtime received or acted on the context. Focused test results and publication status belong in the implementation result; this report makes no independent deployment claim.

Board acceptance is not proof of personal authorship: TaskQ workers share the configured board credential. A trusted board login or accepted comment proves the board record, not that the platform user personally wrote its content. Preserve supported user-history evidence separately when an action requires it; neither a label nor a reconstructed question creates that evidence.

## Remaining design: answer-to-acceptance reconciliation

Before resuming an answer that changes scope or waives a criterion, the PM needs one reviewable reconciliation operation:

1. Read the current ask revision, accepted owner-authored answer, current task body and affected stable `A<n>` identifiers. Preserve the exact source and scope; distinguish the owner's words from agent summaries.
2. Prepare only the criterion changes already entailed by that decision. Preserve unrelated criteria, evidence limits and dependencies. A changed product intent, ambiguous waiver or newly requested risk remains an owner question.
3. Fence the operation against the shown question revision and the body revision/content used to prepare the patch. An ask revision alone does not detect a concurrent body edit. Refuse or reconcile stale input without a second mutation.
4. Update and read back the acceptance text before releasing answer/resume. Link the answer, question revision and affected criteria in a durable board record. Retry through the existing operation identity; do not append another decision on recovery.
5. After a partial failure, re-read body and accepted log before deciding whether to finish the existing operation. Never infer success from message delivery. Do not resume with a silently half-applied scope change.

GitHub/GitLab body update and append-only task comment are separate writes, so this is a recoverable sequence, not an assumed atomic transaction. The expected-body fence and recovery record need explicit contract and board-adapter design before implementation. Do not add a second local queue or silently reinterpret free text as a criterion patch.

Technical choices already belong to the worker: use the recommended option. Relaying a previously accepted, unambiguous product decision and aligning its text is PM work. New product, scope or visual choices still reach the owner. Stale questions should be reconciled from current board state and the exact accepted decision, not answered by keyword similarity or a blanket automatic yes.

The PM's classification before forwarding is:

| Question class | Next action |
|---|---|
| Technical choice within accepted scope | Worker takes the recommendation; PM resolves an escaped technical ask. |
| Same decision, unchanged scope | Cite the exact earlier answer and question; reconcile stale criteria before resume. No new owner question. |
| Stale card or uncertain delivery | Read current state and reconcile the original operation identity; do not apply an old option number to a newer card. |
| Missing host, dependency or evidence | Use the existing blocker/dependency mechanism or report the specific evidence gap; do not disguise an engineering wait as a product decision. |
| New or materially conflicting product/scope/visual choice | Present the unresolved choice and explain what changed since the earlier answer. |
| Mandatory access, disclosure or publication approval | Preserve the exact action-time boundary and supported evidence; never infer approval from a recommendation. |

This is a decision procedure for the PM, not a new inferred `question_kind` grant. Deterministic classification by keywords would risk treating a consequential action as an ordinary technical choice. The current patch does not add that classifier.

## Existing work and boundaries

Read-only board search and exact issue fetch found no active task owning this reconciliation. Current TaskQ work #2338 concerns tray replies and worker-history qualification; #2377 concerns contract compaction. Recheck active work before filing or starting implementation because this is a point-in-time observation.

Reuse closed [#887](https://github.com/taskqorg/taskq/issues/887) for operation IDs, revision fencing and readback; [#1785](https://github.com/taskqorg/taskq/issues/1785) and [#1861](https://github.com/taskqorg/taskq/issues/1861) for concurrent-answer UX; and [#2202](https://github.com/taskqorg/taskq/issues/2202) for rule-gate interpretation of numbered answers with notes. Historical authorization propagation [#209](https://github.com/taskqorg/taskq/issues/209) and PM/ARM continuation [#601](https://github.com/taskqorg/taskq/issues/601) are closed, not active ownership or proof of current runtime behavior.

An owner preference against repeated questions does not grant filesystem/network access, authorize a different data recipient, satisfy a required action-time approval, or override auto-review. Preserve exact user-authored evidence across supported handoff routes; an agent-authored summary is not that evidence. After a denial, use supported history readback and only the permitted evidence-based retry; do not switch runners, identities or permission policies to evade it. The DEV replay #551 approval boundary is not qualified by the two TaskQ examples above.

## Verification needed for the remaining operation

Use a small executable trace covering accepted waiver, unchanged decision across worker/reviewer restart, concurrent body/card edits, edited or rejected answers, retry after each partial write, and a later owner decision replacing an earlier one. Verify unrelated criteria and formal approval gates remain intact. Native handoff/readback and approval-reviewer behavior require separate supported evidence; deterministic tests alone cannot qualify them.
