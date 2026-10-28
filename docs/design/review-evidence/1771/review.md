# UX07: reading and answering first

Conversations now opens with a plain thread heading, a compact activity label, reading content and the shared composer. One collapsed Details disclosure holds rename, window ownership, archive actions, separate-request creation, project selection, launch handoff, watch and input limits. Task answers hide unsupported Attach and retain text-only and number-as-text guidance in Details. Supported files, source choices, validation, delivery failures and recovery stay directly available.

R13 amended (R8 wording shortened without changing its golden-trace, qualification or size protection): `taskq.md` Product records the layout before implementation; C3/D7 in `ai-workspace.md`, `tray-workspace.md` and `visual-style.md` carry the detail. R1/R9 authority, native transcript ownership and R11 execution preservation are unchanged. R12 separates browser observations from installed acceptance.

## Reproduction and review set

Baseline: `28b2f9252b86fe72ccfc2c2aa76ffe11a7714359`. `TASKQ_PHASE_1771=before node tests/conversations-declutter.browser.cjs` failed with `UX07: separate-request creation must be collapsed by default` (`true !== false`). Three baseline images show the project selector, runtime/host paragraph, transport instructions, File limits and New from draft around the input. The same test passes after the change, including six width/theme cells and supported touch targets.

The ten-screen set below follows this task's A5 width/theme requirement. It intentionally includes size/theme variants requested by A5. All images use the real bundled workspace with invented IPC, 20 px system UI text, 1 CSS pixel scale and a 740 px usable height. They establish browser layout, not native delivery. `manifest.json` pins the source blobs, bundle, renderer and image hashes.

| Screen | Purpose |
|---|---|
| [Before 350 dark](before-350-dark.png) / [After](after-350-dark.png) | Narrow reading space and fewer default controls |
| [Before 620 light](before-620-light.png) / [After](after-620-light.png) | List/Back threshold and short destination line |
| [Before 940 dark](before-940-dark.png) / [After](after-940-dark.png) | Stable heading with independent sidebar |
| [After 350 light](after-350-light.png) | Narrow light theme |
| [After 620 dark](after-620-dark.png) | Threshold dark theme |
| [After 940 light](after-940-light.png) | Wide light theme |
| [Task answer](after-question-940-light.png) | Question choices and composer without disabled Attach or transport paragraphs |

Visual inspection: Pass for the represented browser layouts. The project picker, rename/window/archive actions, watch, file limits and separate-request button no longer take default reading space. Palette A, bundled glyphs and type remain unchanged. The fine-pointer Details target is at least 32 px; coarse-pointer Details, Attach and Send are at least 44 px. Keyboard Enter opens Details. Long input, Send reachability and disclosure scrolling retain the previous behavior.

## Preservation journeys

The original 72-group audit remains the inventory. The affected presentation groups map to these passing browser/component journeys; native effects are outside those passes.

| Audit groups | Existing journey and retained protection |
|---|---|
| C01ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“C03, C16, C18; A01, A07ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“A09 | `messages.browser.cjs`, `conversations-selection.browser.cjs`, `conversations-stable.browser.cjs`: unified owner/task/event selection; per-thread drafts, source IDs, explicit read/archive/restore, late receipts, stale revisions and list/reading positions |
| C04ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“C10, C12ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“C17, C24 | `conversations.browser.cjs`: explicit project/runtime, rename, History fills only, durable draft/caret fixture, supported file bytes/preview/removal, immutable launch, separate-request fork, detach owner, cancel/reconcile, archive/Undo/Restore, IME/keyboard/reopen; unreadable files and unknown outcomes preserve drafts |
| C05, C19ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“C22 | `dialogue.browser.cjs`: same-session reply/refusal, transcript paging/live append, visible unknown/work activity, watch create/edit/remove with minimum interval/event validation, narrow/large text and fixed Send |
| C20ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“C21 | `conversations-render.browser.cjs`: both provider-shaped 10,000-record histories, bounded mount, exact record order/anchor within 2 px, selection, skipped pages and obsolete read rejection; six adverse geometry fixtures |
| C11, C23; A03ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“A04, A06ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“A10 | `workspace-communication.browser.cjs`, `tray-answer.browser.cjs`, `conversations-stable.browser.cjs`: original navigation, same-revision text/choice semantics, limits, literal numbers, recovery, operation identity and thread-local receipts; session-link validation retained |
| X01, X03, X05ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“X06 | `task-workspace.test.cjs`, `pm-composer.test.cjs`, `check-web.cjs`: released bundle/stylesheet, pinned shared source hashes, PM schema/receipt correlation and safe native links; no export or adapter changes |
| UX07 layout across the groups above | `conversations-declutter.browser.cjs`: default collapse, keyboard access, rename, destination and activity, preserved choices, file-answer limitation, 350/620/940 widths, both themes and 32/44 px targets; opening controls never dispatches work |

Fixture changes replace obsolete visible-placement/helper-copy expectations only. They retain the original effects, payload identity, file bytes, validation, source choices, stale guards and receipt assertions. The new regression protects the reproduced clutter; no permission, model, transport, storage or DashQ interface is changed. Workspace CSS stays in `workspace.html`; released `task-workspace` and `pm-composer` source hashes are unchanged. A live DashQ deployment is Not tested.

## Acceptance status

- A1ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Å“A5: implemented; focused browser/component checks pass, with the selected candidate gate recorded in the result. UX07 is done at this source/browser boundary; owner visual acceptance remains the owner's decision.
- A6: merge and the owner's Mac dev-install pickup remain pending publication. No host update or service operation is performed by this task. Native Windows: **Not tested**. Native Mac and real-provider turns: **Not tested**.
- Native snapshots used for the README manifest, if produced, are capture instruments; they do not qualify native input, window ownership or delivery.

## Candidate gate observation

`python3 scripts/check.py --base 28b2f9252b86fe72ccfc2c2aa76ffe11a7714359` first ran in 456.531 s on Windows with a cold debug cache: 543 unit cases passed, 21 failed, 37 ignored. The added Product line exceeded R8's 60 KiB bound; the wording was shortened without weakening that test. Host installation/capacity/lifecycle cases encountered worker-authority refusals, including `a worker cannot change host policy`; Windows security restore required an unrestricted operator. These remain sandbox-blocked, not passes. Two Codex App IPC deadline fixtures also failed; their cause is unverified. No permission guard or timeout was changed. Merge-result CI remains the publication gate.

Canonical `npm ci` also changed the bundle relative to the initial browser-tool setup. Baseline and candidate captures were regenerated against the locked dependencies, and the manifest pins both rebuilt artifacts. The browser fixture setup now lives separately in ignored scratch state so it cannot alter project dependency resolution. This is an evidence correction, not a dependency update.

After the wording fix, `cargo +stable test --locked --bin taskq core::tests::every_table_row_has_a_golden_trace` passes; the root contract is 61,435 UTF-8 bytes, within 300 lines and 60 KiB.

Final bounded verification: all 25 `tests/*.test.cjs` scripts and nine affected browser instruments pass against locked dependencies. The resumed component/browser sequence took 176.922 s; previously passed component cases were retained. The visual-style provenance guard first rejected the changed specification hash, then passed after its manifest entry was refreshed; source/asset checks remain enabled. `cargo run --release --locked -- selftest` in an isolated offline installation passes 99 fold traces, owner-text formats and nine committed telemetry observations; it performs no board read. `docs/shots/update.sh` and `docs/shots/check.sh` pass. Readback confirms that the README capture binary hash matches and its embedded workspace bundle equals the browser evidence bundle. The full local gate is still not a pass: host tests are sandbox-blocked and the two IPC timeouts remain unknown; integration tests were not reached.
