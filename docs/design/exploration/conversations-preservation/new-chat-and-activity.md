# New chat and task activity: intent before controls

The plus icon did not explain what it created. Work sessions mixed provider terminology with a different user goal. Icon compactness cannot replace a recognizable intent. This correction is a prototype proposal; canonical product decisions and native implementation remain gated by R13. Task #1738 stays held.

| Goal | Entry | What happens | What does not happen merely by opening it |
|---|---|---|---|
| Continue discussing the project | Select an existing chat by topic | Return to its messages, draft and recorded runtime | No new conversation, send or acknowledgement |
| Start a separate topic with the PM | New chat | Save a new independent draft; show the registered project and first-message field | No agent session until the first send |
| Start that new chat | Start chat | Persist one immutable first request; show waiting/starting or recovery; then continue in the same chat | No second request on a timeout or ordinary navigation |
| Inspect an executor/reviewer | More → Task activity | Show agents grouped under their source task, with role/state/runtime/host | No worker creation, session creation or input |
| Inspect its existing native conversation | Open agent chat | Open the source-verified Codex/Claude destination | No input or invented remote/headless link |
| Resolve a question found there | Answer question | Open that exact task/root/board in Conversations | No answer until the owner chooses or sends |

New chat is a labelled plus action beside the chat list, with a narrow-window Chats/Back control. The first-message view keeps the explanation, project and input together; at short heights the explanation scrolls while project/input/Start chat remain reachable. Starting, unknown, interrupted and failed delivery retain the native receipt/reconciliation/cancellation actions and submitted bytes behind First message and details. Existing chats show Ready/Working/Draft rather than internal launch vocabulary. Archive, rename, files, detach, command history and New from draft remain.

Task activity replaces the visible Work sessions label while retaining its route and source identities. It is an inspection page, not a second chat inbox. Executing, reviewing and waiting for the owner have different labels/colors. Unavailable links have a reason; stale sources remain unknown. Back returns to the same PM draft. An ordinary task title opens its Board; it does not create an empty reply form.

Checks cover new-draft creation without launch, visible project/Start chat, Back preserving the draft, first send creating exactly one request, the first reply appearing in that chat, the narrow 350×430 / 20 px start action and exact task-question routing from activity. Existing 144 geometry cells and preservation checks remain applicable. Screens 07/08 show these two distinct decisions; native/provider execution is still Not tested.
