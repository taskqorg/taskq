# Native transcript evidence (#2176)

Conversations reads every runtime through AgentQ `history` ([D1/D4](../../design/ai-workspace.md)). This page records what the product reader did on retained native transcripts on the Mac host. No model turn was started for it; every read was read-only.

## Method

`cargo test --locked --bin taskq native_transcripts -- --ignored --nocapture` with `TASKQ_NATIVE_SOURCES` naming one retained transcript per runtime (`format=session=path`, paths not recorded here). For each source the instrument reads all backward pages from the file end, then all forward pages from offset 0, and requires unique ordered record identities and identical items both ways. It prints counts only.

## Result

Host: macOS 27.2 arm64, branch `taskq-2176`, AgentQ `ce1e2788`.

| Runtime | Format | CLI version | Source | Pages | Items | Tool results | History | Native |
|---|---|---|---|---|---|---|---|---|
| Claude | `claude-native` | 2.1.316 | 0.99 MB worker transcript | 2 | 20 | 9 | supported | untested |
| Codex | `codex-rollout` | 0.159.3 | 1.96 MB reviewer rollout | 2 | 46 | 19 | supported | untested |
| Antigravity | `agy-stream` | 1.4.12 | 0.15 MB #2157 attempt log | 2 | 81 | 35 | limited | untested |
| Antigravity | `agy-stream` | 1.4.12 | 2.63 MB #2157 attempt log | 14 | 1072 | 507 | limited | untested |

`limited` reason (AgentQ): completed steps may omit text; deltas/result only; unknown shapes retained raw. The 0.15 MB Antigravity log has no final result, so it shows tool lines and no assistant text. `untested` is AgentQ's own native qualification for the history API; TaskQ shows it and does not upgrade it.

## Defect found and fixed

Before: the Codex rollout failed the both-ways check at item 5 of 28. The `exec` call and its output fell into different backward pages, so the backward read showed the call without a result body and the forward read showed it with one (32 KiB). The view depended on page boundaries.

After: each tool result is one item at its own record, opened on request. All four sources pass. `transcript::tests::tool_results_are_page_independent` keeps the protection with 150 split call/output pairs and checks output masking.

## Not covered here

- Native window switching, Reload, Close and focus in the Conversations window: owner-observed per [testing](../../testing.md); browser instruments cover the source behavior.
- Lifecycle and unknown normalized events stay in the exact source only (D4); no lifecycle row is rendered.
