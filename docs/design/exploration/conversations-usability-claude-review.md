# Claude technical review of the usability plan

Consultation requested by the owner on 2026-10-28. One native Claude CLI print turn, tools disabled, permission mode `dontAsk`, effort `max`, no model override. Returned model: `claude-opus-5-5[1m]`; session `c2a51858-dd05-42ff-a1f2-8dac8ad1602d`; successful result. This review read the supplied plan and brief, not the live native application or provider documentation. Statements marked verify below are hypotheses, not established capabilities.

## Disposition

| Advice | Disposition |
|---|---|
| Separate per-operation delivery from session activity; slow is elapsed time only | Adopted in the candidate's message/operation feedback and session activity. The prototype uses scripted receipts and events, not a native protocol implementation. |
| Inline menu attention; one manual scroll authority | Adopted. No wheel forwarding; reading viewport uses explicit anchoring. |
| Same-ID reconciliation and correlation scoped to thread/session/turn | Adopted for the simulation. Native matching must use dispatch boundary and provider identity, never text alone; restart/mismatch stays unknown. |
| Recheck short/empty-page progress and reject obsolete epochs/generations | Added to the implementation plan and acceptance. The existing Rust reader already doubles its read window for long records up to 16 MiB, so a 256 KiB line stall is not a proved current bug. |
| Cap forward IPC and keep a measured contiguous page window first | Recommended production starting point. The prototype uses a known-count deterministic fixture to exercise bounded DOM and thumb navigation; it is not evidence of a production transcript index. |
| Generation identity and grouping across page seams | Accepted as required investigation; exact Claude/Codex identifiers and generation detection need native fixtures. Inode/hash/size recipes are proposals, not a complete replacement detector. |
| Truncate in Rust or render unknown/oversized placeholders | Not adopted as a silent change. D1/D4 and source preservation apply: visible folding may link to full content; historical unknown records remain skipped under the current contract. Unsupported oversized records remain explicit. |
| Infer definite non-delivery from a missing transcript record after exit | Rejected as sufficient proof: incomplete reads, format drift and writer semantics can still leave uncertainty. A definite adapter refusal is safe; absence alone never permits replay. |
| Provider capability matrix, native accessibility and Select All/find limits | Unverified until checked against current CLIs and WKWebView/WebView2. No cancellation/stop button is exposed without a proven capability. A loaded-window limitation must be disclosed; this prototype does not claim native global find. |
| TanStack Virtual only after measurement | Agreed. No new production dependency was added. Its documented anchoring features are source research, not TaskQ renderer qualification. |

## Claude response

Tags: **[fact]** comes from your brief. **[verify]** is platform or provider behavior from my knowledge that I have not checked here. Untagged text is analysis or proposal.

## 1. Blockers and corrections

**Blockers**

1. **The history loader can stall.** IntersectionObserver fires only when intersection changes [verify]. After a short or empty page the sentinel stays visible, so no callback fires. This is the UX06 flaw again with a new API. Re-check the sentinel after each page commit. Invariant: every backward read lowers `lo` or returns `atStart`/error. Set a byte budget per fill burst, then show Load earlier.
2. **A line longer than the 256 KiB window** (inline base64 image, huge tool output [verify]) yields zero complete lines. The reader must emit an `oversized` placeholder and advance. It must never get stuck and never report a false `atStart`.
3. **`after` up to 16 MiB [fact]** is one frame-blocking IPC/parse/render unit.
   - Cap it at one page.
   - Detached reader: track `eof` only and show New output without a count.
   - Gap larger than the window: re-tail; the middle loads upward on demand.
   - Normalize and truncate in Rust; send no raw lines over IPC [verify current payload].
4. **A byte offset is not identity across generations, and a line is not a message.**
   - Claude splits one assistant message into lines that share `message.id` [verify]. A page seam can cut that group.
   - Merge adjacent same-group records after load. Anchor on a line-start record below the seam.
   - Generation = file identity + size never shrinks + hash of the first 4 KiB. A change voids all offsets and anchors.
   - A request epoch (thread, generation, direction, cursor) drops stale replies.
5. **The delivery table mixes two state machines**: per-submission delivery (keyed by persisted turn id [fact]) and per-session activity.
   - `working` = taskq's own writer is alive (one writer [fact]). Never infer it from transcript growth.
   - Writer not owned by taskq = `unknown`.
   - `slow` is only an elapsed-time label. It triggers nothing.
6. **Correlating by text alone is unsafe.** "ok" or "1" can repeat, and the CLI rewrites input. Native "user" records include non-human input (see table) [verify], so the adapter sets `human`.
   - Key: one in-flight turn per session + the first human record after the dispatch point.
   - Capture `eof` before spawning the writer. After a restart, compare turn time with the native timestamp.
   - Text only confirms the match. On mismatch keep both rows; never merge them.
7. **Moving `unknown` to `refused` needs proof**: the writer has exited AND the transcript was read past the dispatch point AND it has no human record. Otherwise the state stays `unknown`. Re-send is an explicit user action with a new turn id and the same runtime; the old row stays.

**Corrections**

8. **Anchored preview.** A wheel over an overlay chains to the viewport/root, not to the list behind it [verify]. To pass Q-A1 the list would have to be the root scroller, plus dismiss/reposition on scroll. The only other route is wheel forwarding, which the plan forbids. Use the inline disclosure only.
9. **The two engines differ.** WebView2 anchors scroll natively; WebKit support is absent or partial, depending on version [verify]. Mixing native and manual correction gives different shifts per engine. `overflow-anchor:none` on the reading viewport is mandatory.
10. **Keyboard, accessibility and copy.**
    - WebKit scrollers are not Tab-focusable by default [verify]. Give the viewport `tabindex=0` and a label.
    - Use `role=feed`, `article` with `aria-posinset`, `aria-setsize=-1`, and `aria-busy` while loading. Use a polite live status only for New output.
    - Native find and Select All see only mounted pages; say so in the UI.
    - The scrollbar thumb covers only the loaded window and jumps on load/evict; adjust Q-A9.

R13: the D4/C3/M3 spec edits come before code. D1: persist only the anchor `{gen, offset, ordinal, px}`, as navigation metadata.

## 2. Shared shape (proposal)

```ts
// Rust adapter output, one IPC page
Page { gen, lo, hi, eof, atStart, recs: Rec[] }
Rec  { id /* gen:lineOffset:ordinal */, group?, nativeId?, at?,
       role: 'you'|'agent'|'tool'|'system',
       kind: 'text'|'activity'|'thinking'|'compaction'|'attachment'|'oversized'|'unrecognized',
       human: boolean, text /* truncated */, bytes, more?: [lo, hi] }
// UI-local, keyed by persisted turn id
Submit   { turn, thread, session, rev?, dispatch: { eof?, time },
           state: 'pending'|'accepted'|'recorded'|'refused'|'unknown',
           reason?, match?: string /* Rec id */ }
Activity { session, state: 'idle'|'working'|'unknown', since }
```

States:
- `pending`: payload frozen, dispatch running.
- `accepted`: the writer took the input. This is not a reply.
- `recorded`: a human record matched. The local row becomes the native record with a badge.
- `refused`: definite non-delivery with a reason (busy, revision, validation, verified absent). The draft stays editable.
- `unknown`: no definite result, or a restart mid-flight. Check delivery only reads; it never sends.

Meta lines emit no `Rec` but still lower `lo`. Unrecognized record types render as one collapsed line, so format drift stays visible.

Capabilities (all cells [verify] against current CLIs):

| Capability | Claude | Codex |
|---|---|---|
| Record id | offset + `uuid` | offset only |
| Group | `message.id` | none |
| Human marker | `type:user` minus tool_result, `isMeta`, compaction, sidechain | `event_msg` `user_message`; injected context excluded |
| Duplicates | none | `response_item` + `event_msg`: render one |
| Timestamps | per line | per line |
| Live output | transcript append; no token stream | same |
| Receipt | process accepted | process accepted |
| Cancel | absent | absent |

Board answers: comment id, existing same-ID reconcile, no transcript. Show a control only where the capability exists. Otherwise show nothing or a one-line limit, never a disabled imitation.

## 3. Prototype must-test

1. **Menu:** real trackpad flick and wheel, starting over the question summary and starting over the list and crossing it, in both engines. The list moves; there is no inner scroller.
2. **Hostile fixture per provider, built from real samples:** 100 MiB; a 5 MiB single line; 3 MiB of assistant text; 2,000 consecutive hidden lines; a grouped message across a seam; multibyte UTF-8 at a seam; Codex duplicates; Claude sidechain and compaction; plus a 3-record file. Traverse to the start by wheel, keyboard and thumb. Pass: offsets have no gaps or duplicates, mounted nodes plateau, the loop terminates.
3. **Prepend under motion:** momentum flick, thumb drag, font change, resize, late image. Anchor holds within ±2 px with no second jump, in both engines.
4. **Detached append:** 1 line per 100 ms plus a 20 MiB burst. The reader does not move and IPC stays bounded. Jump to latest re-tails. At the end, the view follows. Composer growth keeps the end pinned.
5. **Selection and screen reader:** select across 3 messages, scroll until prepend and evict happen, then return. The selection is intact and Cmd+C order is right. VoiceOver/Narrator read You/Agent, prepend is silent, and Tab reaches the viewport for PageUp/Down.
6. **Stale reads and generations:** switch threads A, then B, then A, with delayed reads. Truncate or replace the file while it is open. Stale pages are dropped, a notice and Latest appear, and the anchor restores after reopen.
7. **Delivery faults:**
   - kill the writer before the record, and again before the reply;
   - IPC timeout;
   - restart while pending;
   - double Send from two surfaces;
   - "ok" sent twice;
   - text rewritten by the CLI;
   - late receipt for another thread.

   Pass: one row each, zero second dispatch, no `refused` without proof.
8. **Composer input:** Japanese IME Enter in WKWebView (Safari can report `isComposing=false` with `keyCode` 229 [verify]); Shift+Enter; the draft survives a question revision and a thread switch.
9. **Idle cost:** an idle thread polled for 60 s causes zero DOM mutations (MutationObserver) and zero re-renders.

## 4. Simplest implementation (proposal)

- **Layout:** grid `auto minmax(0,1fr) auto` for heading, reading viewport and composer. The reading viewport is the only scroller in the pane.
- **Page window, no virtualizer.**
  - Keep a contiguous byte window of at most K pages, with K set by measurement. Each page is at most 256 KiB and at most N records.
  - Top changes (prepend, evict-top, seam merge) go through one anchor restore: `useLayoutEffect` after the splice, and ResizeObserver for later height changes. Bottom changes need no correction.
  - Nothing outside the window has geometry, so no height cache is needed. Evicted pages are re-read by cursor.
  - A non-collapsed selection suspends eviction; a hard cap pauses loading.
- **Reader:** existing reader plus an `after` cap; provider adapters live in Rust.
- **Rendering:** keys = `Rec.id`, memoized rows, one signal per page. A poll with no change writes nothing.
- **Feedback:** key it by (thread, turn/operation). This fixes UX03.
- **TanStack Virtual:** only if the page window misses a measured budget. The plan's claim that it handles chat anchoring needs checking [verify].
