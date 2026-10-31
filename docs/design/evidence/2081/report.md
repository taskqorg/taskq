# Held and Later state icons — #2081

A3 native check: the macOS 27.0.1 system WKWebView renders the candidate project page at menu width (320 logical points, 2× backing), 16 pt text, with one invented project holding one Held and one Later task. Base `98478c711b525d436ada9107161b6d21cce409f0` plus this task's source change.

Command: `TASKQ_INSTALL=<isolated fixture>/install TASKQ_INCIDENT_REPORTS=off ./target/debug/taskq tray --snapshot <out> --route project` — exit 0. The fixture is an isolated temporary installation; it never reads or writes the owner installation.

| Image | Theme | SHA-256 |
|---|---|---|
| [light.png](light.png) | Light | `654d8cc127f4d4385836d2bc83bd9ee2e5207f3daa8e18e2cb994cae5d12f9c9` |
| [dark.png](dark.png) | Dark | `ef8f966ae4515d64f9a92e0a6dbc6a32bad65fa4366e04776170638e38c7a801` |

Both images were opened at native resolution and inspected. The Later row and counter show circle-pause; the Held row and counter show circle-dashed; the two marks differ in both themes. The counters read `?` because the fixture carries no display-model counts; that is the unknown rendering, unrelated to this change.

These are native renderer snapshots, not window-server captures of the installed application.
