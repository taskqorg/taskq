# #1730 Mac continuation: four View captures

R12 / § 7: production UI source candidate `7c0c52d5e7a1209c09f40c1c5ede7f577c239f32`, rebased on `65205cd45395df6c9e7bbab955e5f9a9378fab5a`. MP11/P-W4 own the intentional three-column/exclusive-project/naming changes (R13); the shared chrome, palette, icons and saved-view schema are retained.

The macOS 27.0.1 Apple Silicon system WKWebView renders the candidate View page at 480×600 logical points, 2× backing. The signed debug binary has SHA-256 `ed396e4656aa893c220a9ab144cd0e81518790082b427a20a4e9f2813494de52`. It adds only the existing [evidence fixture patch](../../../review-evidence/native-1730-2026-10-27/views-fixture.patch) to `src/tray.rs`; that patch was reversed after capture and is not part of the deliverable. The four scenes and eight invented projects reuse [fixture8.sh](../../../review-evidence/native-1730-2026-10-27/fixture8.sh).

Command: `TASKQ_INSTALL=<isolated fixture>/install TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --snapshot <out> --route views` — exit 0, including the native horizontal-overflow check. The fixture is an isolated temporary installation; it never loads or writes the owner installation or saved-view store. The earlier [owner-store before/after hash](../../../review-evidence/native-1730-2026-10-27/report.md) is historical evidence, not a fresh read of that store in this continuation.

| Image | Scene | SHA-256 |
|---|---|---|
| [01-views-16pt.png](01-views-16pt.png) | 16 pt: three saved views and both complete grids | `b6a2e54c3de23830ae7f3ebf5ffcb8c32561880cbc8e8c7b7fcca28633bb8f5c` |
| [02-naming-undo-16pt.png](02-naming-undo-16pt.png) | 16 pt: naming draft and Undo | `209140e038deb8ef174da824e84ba8792c4b22af2427925ad79733e26190c639` |
| [03-long-20pt-top.png](03-long-20pt-top.png) | 20 pt: long names, top; F1/F2 corrected | `4000c95b293a3aa825dea33b88185ce3500a2bde59c60b6530defee42b68917c` |
| [04-long-20pt-end.png](04-long-20pt-end.png) | 20 pt: end of the only content scroll | `14b32927c23f206ce23bc025c54fd3077b6599894edb12236e75d6f829c20b38` |

All four images were opened at native resolution and visually inspected. At 16 pt the three saved views and both grids fit together, including naming and Undo; all icons and labels are complete. At 20 pt the fixed Close/footer stay visible, the content scroll reaches Held, and text wraps without horizontal overflow. The baseline [F1/F2 failures](../../../review-evidence/native-1730-2026-10-27/report.md) are corrected: All projects has a compact visible label at the top, and the long header ends in an ellipsis. The browser instrument separately passes project membership, exclusive clicks, naming drafts, Save/Reset, Delete/Undo, focus and keyboard checks; it retains the explicit one-scroller assertion.

These are native renderer snapshots, not window-server captures of the production installed application. Real window chrome, pointer input and window-raise behavior remain unknown; native click/raise is owner-observed under the UI acceptance hard gates. No production installation, service restart or owner preference edit was performed. Owner visual acceptance and merge-result CI remain separate.
