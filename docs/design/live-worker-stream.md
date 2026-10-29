# Live worker session stream

Experiment for [#1825](https://github.com/taskqorg/taskq/issues/1825): how Conversations shows what a task worker session does and how the owner writes back into it.
Status: experiment and prototype. Nothing here changes [taskq.md](../../taskq.md) or [ai-workspace.md](ai-workspace.md) D1–D8; adoption needs the owner decision this experiment ends with.

Probes ran on 2026-10-29 on one Windows 11 host with Claude Code 2.1.312 and codex-cli 0.162.1, against real sessions with small models and short prompts.
Probe scripts stayed outside the repository; every number below is a measured value, not an estimate.

## A1. Capability matrix

"Today" is how taskq launches workers now: Claude with `--bg` (resumed with `--bg --resume`), Codex with `exec --json` (resumed with `exec resume`).
The other two columns are the interactive surfaces each runtime offers if taskq owned the session's input.

| Capability | Claude `--bg` (today) | Claude `stream-json` in and out | Codex `exec --json` (today) | Codex `app-server` |
|---|---|---|---|---|
| Messages | Native transcript only; no stdout to the service | `assistant` events with text blocks | `item.completed` `agent_message` | `item/agentMessage/delta` (23 deltas in one probe) and completed items |
| Reasoning | Transcript `thinking` blocks are stored empty with a signature | Same; the stream adds only thinking-token estimates | No reasoning item, also with `model_reasoning_summary=detailed`; only `reasoning_output_tokens` in `turn.completed` | No reasoning notification or item observed |
| Tool calls | Transcript `tool_use` blocks | `tool_use` blocks in `assistant` events | `command_execution`, `mcp_tool_call`, `file_change` items with `item.started` and `item.completed` | `item/started` and `item/completed` per item |
| Command output | Transcript `tool_result` | `tool_result` blocks in `user` events | `aggregated_output` on the completed `command_execution` item | `item/commandExecution/outputDelta` while the command runs |
| Edits | Transcript `Edit` tool call with old and new strings | `Edit` `tool_use` input; the result carries `structuredPatch` | `file_change` item with path and kind only, no diff text | `file_change` item (same shape) |
| Approvals | None: no prompt reaches the service; a prompt-requiring call fails in the session | `--permission-prompt-tool stdio`: `can_use_tool` control request, answered `allow`, the call ran | None: non-interactive, no approval request | `approvalPolicy: untrusted`: `item/commandExecution/requestApproval` server request after 6.69 s, answered `accept`, the command ran |
| Interrupt | Stop the process only | `interrupt` control request: success, transcript records `[Request interrupted by user]`, result `error_during_execution` | Kill the process only | `turn/interrupt`: turn completed 0.32 s later |
| Mid-turn input | No stdin; next turn only via stop and `--bg --resume` (the existing steer path) | Yes: a second user message sent at 4.01 s was delivered at the next tool boundary (12.63 s) and answered in the same turn | No: the prompt is read once; next turn only via `exec resume` | Yes: `turn/steer` at 9.70 s accepted at once, the user item appeared at 9.88 s while a command still ran, answered in the same turn |
| Native transcript written | Yes | Yes, 0.06–0.25 s after the stdout event (8 of 8 events matched) | Yes, 0.028–0.036 s after the stdout item | Yes (rollout file of the thread) |

Both runtimes write the native transcript within a quarter second of the event, so the transcript is a usable live source and the stdout stream adds no event kind the owner can read today except Codex command output deltas.

## A2. Prototype: live view

A running task's thread in Conversations shows a "Live session" panel above the existing answer and steer composer.
The panel reads the worker's native transcript on this host through the existing `conversation-dialogue` channel (`action: transcript`) and the existing bounded transcript reader (D4 pages, record identity `at`/`i`).
It polls every second while the panel is in the document and stops when it leaves; it appends only records after the last seen offset and keeps the last 600 items.

Changes:

- `runtime::worker_transcript` resolves a task session's transcript in the service's own Claude or Codex home; ids are validated by the existing readers, so no path comes from the client.
- `dialogue::worker_transcript` returns the page with author labels (A3).
- `tray` adds the plain native `session_id` to the task row for sessions the service owns.
- `conversations.js` renders the panel; `workspace.js` places it in the task page.
- `transcript` masks credentials in tool lines (A4).

End-to-end measurement: a real session ran while a `cargo test -- --ignored live_worker_stream_probe` loop called the same Rust read path once per second, and both printed arrival times.

| Runtime | Events | Delay from native stdout event to relay read |
|---|---|---|
| Claude | 5 tool calls, final message | 0.22, 0.36, 0.62, 0.78, 0.93, 0.23 s |
| Codex | 2 messages, 2 commands | 0.90, 0.13, 0.78, 0.51 s |

Worst case 0.93 s, under the 2 s target. The budget is transcript write lag (≤0.25 s) plus at most one poll interval (1 s) plus the read.

## A3. Owner input

Prototype path: the owner writes in the existing steer composer under the live panel.
The board records a `steer` entry (R1: the board stays the only state), the service resumes the live attempt session with that text as the next user turn, and the live panel shows it within about a second of the runtime recording it.
Answers to asks follow the same rules unchanged: the answer card is the only way to answer, and permissions stay those of the worker (R9).

Authorship: user turns that start with the taskq brief prefix ("Before any work, ") are labeled "TaskQ brief for this turn"; other user turns are labeled "You · via the board"; assistant text is labeled "Worker".
The probe showed a free-text prompt labeled as the owner. The steer delivery itself is the shipped path and was not re-run against a live board task, because that writes to the public board.

Timing: with today's launch modes owner input reaches the session at the next turn, never mid-turn: Claude `--bg` has no input channel and Codex `exec` reads its prompt once.
Mid-turn delivery exists only on the interactive surfaces (Claude `stream-json` input, Codex `turn/steer`), which require taskq to own the session's input (option C below).

## A4. Limits

- Hidden reasoning: neither runtime exposes reasoning text to a client. Claude stores thinking blocks empty with a signature and streams token estimates only; Codex emitted no reasoning item or delta in any mode tried and reports only a reasoning token count. The live view can show "thinking" at most, never its content.
- Sandbox and permission prompts: today's workers are non-interactive, so a prompt never reaches the owner. Probes saw Claude refuse an edit inside its own configuration folder as a "sensitive file" and Codex `workspace-write` on Windows fail to start its helper ("setup refresh had errors") and fail every write, matching the Windows refusal in [worker permissions](worker-permissions.md). Answering prompts live needs option C plus a mapping from runtime approvals to answer cards that keeps R9 presets as the ceiling.
- Session ownership: D3 allows one writer. The live view is read-only and input goes through the board and the service, so the service stays the only writer. An owner attaching to the worker session directly would be a second writer; the prototype does not offer that.
- Cost: the view reads a local file and costs no tokens. Every steer is one more turn of the worker and costs a full turn: a trivial Codex turn read 156,632 input tokens (140,160 cached); a Claude interrupt probe cost $0.0034. Mid-turn input avoids a resumed turn's context reload but still adds tokens.
- Log size: a minimal Claude probe transcript was 320,493 bytes, mostly API request and attachment records; a Codex rollout was 117,033 bytes. The panel reads only new bytes after the last offset in D4-bounded pages and keeps 600 items, so a long session does not grow the page.
- Secrets in command output: command output (tool results) is never shown in the view. Tool lines mask `NAME=value` when the name ends in TOKEN, SECRET, PASSWORD or API_KEY, and token-shaped words with known prefixes (GitHub, GitLab, OpenAI-style `sk-`, Slack, AWS key ids). Not covered: secrets inside assistant or owner text, other formats such as JSON bodies or base64, and local file paths in tool lines. The view is local to the host and none of it is written to the board.
- Reach: the panel works only when the worker runs on the host of the viewing service. Remote workers need the host link to carry the read.
- Codex tool lines currently show raw tool code (for example `exec text(await tools.exec_command(...`) because newer Codex records commands as custom tool calls; a readable summary is follow-up work.
- Author labels are a text heuristic: service-generated next-turn texts other than the brief (for example answer deliveries) are labeled as the owner.

## A5. Architecture options

One runtime-neutral item schema serves all options: `{at, i, role: worker|owner|brief|tool, text, tool?, state?}`, where `at` and `i` are the D4 record identity. The prototype already returns this shape.

### A. Transcript tail (prototype)

The service reads the native transcript of the task session on demand and the UI polls it.

- Pros: no change to how workers launch; both runtimes; the native transcript stays the only record (D1); read-only, so D3 holds; survives service and UI restarts; bounded reads (D4); under 1 s measured.
- Cons: up to one poll interval of extra delay; no live command output for Codex; no approvals, interrupt or mid-turn input; same-host only until the host link carries it.
- Fit: R1 holds (board unchanged, view derived); D1, D3, D4 and D6 hold as written.

### B. Stdout stream relay

The service launches workers in streaming mode and fans their stdout events to viewers.

- Pros: lowest delay (events arrive 0.03–0.25 s before the transcript); Codex output deltas.
- Cons: Claude workers would leave `--bg`, losing Remote Control and the detached lifecycle; the stream is ephemeral, so viewers after a restart still need the transcript, giving two sources to reconcile; no input for Codex `exec`.
- Fit: R1 holds only if the stream is never stored as state; a stored stream would be a second record against D1.

### C. Service-owned interactive session

The service holds the session's input: Claude `stream-json` input with the control protocol, Codex `app-server`.

- Pros: mid-turn input, live approvals mappable to answer cards, interrupt, live output deltas, measured on both runtimes.
- Cons: replaces both launch modes and the resume model; the service must supervise long-lived processes and replay state after restarts; approvals need a new mapping onto R9 presets and answer cards; Codex `app-server` is a version-bound protocol and needs runtime qualification per release; the most code.
- Fit: D3 holds with the service as the one writer; D1 holds if the native transcript remains the record and live events are only a view; R1 holds if every owner input is still a board entry first.

## A6. Recommendation

Adopt A now and keep owner input on the existing next-turn steer path. A meets the 2 s target on both runtimes without changing launch, ownership or the record.
Revisit C as a separate decision only for mid-turn input and live approvals, starting with Codex `app-server`, after A ships.

Follow-up code tasks for A:

1. Ship the live panel: resolve the session from the task's own attempt in the service instead of the client-sent id, add browser tests, and document it in ai-workspace.md.
2. Carry live reads for workers on other hosts through the host link.
3. Summarize Codex custom tool calls into readable tool lines and widen tool-line redaction (JSON headers, bearer tokens).
4. Mark service-delivered next-turn texts (answers, notices) in the delivery so author labels do not rely on text prefixes.
