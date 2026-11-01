# Claude source review

First frozen source manifest: `02b8367904f3282e66133ec3e057fdbaf975475caee276f0c3255f4fc4998963`. Read-only CLI review; no installation or native acceptance.

I found two blocking source defects. I did not run, edit or install anything, and I made no native acceptance claim.

## Confirmed source defects (blocking)

**1. P1. A stale async catalog deletes a just-saved draft or fork. This loses the draft, the queue and the receipt (C-ledger owner identity/drafts, A-ledger exact receipts).**
- **Where:** `src/conversations.rs:204-210` and `src/conversations.js:392,428`.
- **Trigger:**
  1. Create a new chat or fork, or make its first save. The receipt at `:392` sets `c.revision>0`.
  2. The next tray state carries `observed()`'s cached catalog, which is ≤4 s old, has `complete:true` and lacks the new id.
  3. `:204` stamps `at=Instant::now()` when the result is *received*, not when the scan starts. So a scan that began before the save but finished after it still counts as "fresh".
- **Consequence:** `:428` sets `deleted=true` (never cleared) and empties `text`/`sent`/`files`. It drops `queue` and `pending`, and cancels the timer. The UI shows "History permanently deleted". Before this diff the summary was synchronous, so every snapshot after a receipt already included the save. This is a regression.
- **Smallest fix:**
  - Stamp the scan start, not the receive time. Have any conversation act invalidate the cache: `at=None`, discard `pending`.
  - Or never derive deletion from `observed()`: send `complete:false` for observed catalogs and keep deletion on the direct source/revision check.

**2. P1/P2. A background snapshot wipes the sent request text and its file list from a loaded thread (C history/files).**
- **Where:** `src/conversations.js:426`.
- **Trigger:** Any tray state after a thread body has loaded. Lazy summary rows from `observed()` and `tray.rs` carry a stripped `sent`.
- **Consequence:** `c.sent=n.sent` runs unconditionally, before the lazy guard. Within about 2 s, the "Sent request" article loses its text and its files with SHA-256. The guarded merge `if(sent)c.sent={...c.sent,...sent}` only re-adds the stripped fields, so it cannot restore them.
- **Smallest fix:** Drop `c.sent=n.sent` from the common prefix and assign `sent` only in the non-lazy `Object.assign(c,n)` branch. Apply the same check to `request`, `dialogue` and `reply` if lazy rows strip them.

## Confirmed, non-blocking

**3. P2. Pinned overflow is not reported.** report.md requires that selected or pending text over the 32 MiB reader budget is reported explicitly. The trim in `conversations.js` (bodyTrim and the reader trim) only evicts outside the viewport ±2× band and reports nothing. A giant visible or selected record goes over budget silently. **Fix:** when the protected bytes exceed the ceiling, set a status line ("Resident text over budget: N MiB pinned by selection/edits").

**4. P3. `bodyTrim` evicts too early.** It counts the bytes of protected entries against the 8 MiB / 8-source cache, so unprotected bodies are evicted early. This costs re-reads, not data. **Fix:** exclude protected entries from the sum, or report them as in #3.

**5. P3. A failed catalog capture shows an empty catalog.** At `conversations.rs:207`, a capture error sets `Value::Null`, which is cached as a fresh value with `at=now`. For up to 4 s the JS gets `conversations:null` with no reason, and the sidebar looks empty instead of "unknown". **Fix:** on `Err`, store `unknown()` with a reason and do not set `at`.

## Hypotheses (not traced end-to-end)

- **Local archive/restore goes stale.** `workspace.js` `threadProjection` is cleared only on `receive` and `openTask`. A local archive or restore through `conversations.sidebar` redraws with the stale projection until the next snapshot (≤2 s), so row state and counts lag. **Fix:** clear `threadProjection` in `draw()`'s local-state callers, or key it on `sidebarState()` identity.
- **Sidebar group cache key.** It omits title edits, `extra.marks` and outcomes when the `extra.rows` identity is unchanged. Titles and counts may stay stale until the next snapshot.
- **Possible repeated workspace open.** `task-dashboard.js` `model()` sends a workspace open as a render side effect whenever the dashboard is not owned and `selected==='completed'`. This may open repeatedly on redraw.
- **Menu vs centre count parity.** The menu's `attention().needs` uses task `ask` plus undecided reports. The centre's `needs` uses `ownerOutcomes` and workspace rows. `ownerOutcomes.needsReply` is overwritten (`needsReply:t.state==='ask'`) instead of OR-ed across task rows that share one owner conversation, so an earlier ask can be masked. Counts may diverge.
- **Time label width.** `"in <1m"` is 6 characters. It fits the 9ch future slot. Check that the past/future class switch never puts it in the 5ch slot.

## Native-only gaps (Not tested, as report.md says)

- Settings origin Back via `workspace-origin`, which shows the native settings window.
- The 2 s / 4 s catalog cadence under real I/O.
- Installed macOS/Windows timing.
- Codex/Claude adapter delivery.

These look intact in source: authority, the adapters and OS delivery are unchanged, and the `"body"` read checks the revision.

**Verdict:** two blocking defects, #1 and #2. Both break capability preservation for drafts, receipts and sent history. Fix them before Checkpoint 3.

## Disposition

Blocking draft/catalog and sent-history findings reproduced by `tests/conversations-cache.test.cjs`: both fail before correction and pass after. Cache epochs fence pre-mutation absence; source scans retain their start time. Later exact-source review pending.

## Second review attempt

Manifest SHA-256: `4281e35f56a29a47d791bd4fcbb43febf451a3d8fd45df61790514966bc6a6f8` (15 modified source files). The default configured Claude CLI was read-only (`Read,Grep,Glob`, `dontAsk`); no model, effort, login, permissions or account policy changed. It ran 12 turns and returned an error after 71,501 ms:

> You've hit your weekly limit · resets 12pm (Asia/Bangkok)

No review verdict was produced. The reset message supplies no date, so no reset deadline is inferred. The owner-configured local account list has only the enabled default Claude account. No fallback was taken. A8 exact-source review remains **Not tested / blocked by quota**. Later source corrections include archive/count scope, the duplicate brief time label, result-reader identity, eviction/reload cursors, immutable requested-anchor validation, worker source loss and pending-history tail preservation; they are not certified by this aborted review. The final candidate requires a complete Claude review.

The two first-round blockers have explicit failing-before/passing-after component checks: stale catalog absence cannot erase a confirmed draft/operation, and a thin snapshot cannot erase immutable sent text/files. Epoch fences bind source receipts and scan-start freshness. Pinned overflow is now disclosed; unknown collection returns an unknown envelope. The conservative body-cache partition retains a combined maximum of eight inactive sources / 8 MiB, with protected live edits/receipts retained.
