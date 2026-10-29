# Loaded-project counters

Task [#1840](https://github.com/taskqorg/taskq/issues/1840), taskq.md § 4 (R8/R13).

The isolated browser fixture contains one loaded board with seven open tasks and one failed board with the cause `no access to the repository`.

- [Before](before.png): `All tasks ?` and the generic Partial advice.
- [After](after.png): `All tasks 7`, loaded state counts and one plain `Broken: no access to the repository` line.

`node tests/partial-project.browser.cjs` failed before the dashboard change (`All tasks ?`, expected `All tasks 7`) and passed after it. It also checks search-scoped totals, absence of the generic advice and unknown counts when only the failed board remains. `node tests/display-model.test.cjs` reproduced the same defect through the actual controller and passed after the fix.

These are Chromium headless screenshots of the bundled menu/dashboard with invented inputs. They do not establish native window behavior or live repository access. No production credential or service was changed.

The credential regression opens one board through the production provider registry and substitutes only its HTTP endpoint with a local fixture. The cached-token baseline failed because the old token's pause blocked the next read after replacement. With credential refresh enabled, `cargo test --locked --bin taskq next_read_uses_replaced_host_token` passed: the fixture observed both invented credentials in order, the old pause file remained, and deleting the token refused the next read.

Windows checks of Rust candidate `49541499a88cc6b0c222265753a7d258fac48ae4`:

- `RUST_TEST_THREADS=2 cargo +stable test --locked`: 617 passed, one failed, 38 ignored. `host_control::tests::saved_unit_accepts_only_logon_and_one_repeat_trigger` hit `host.install.definition: process timeout`; the full suite is not a pass. The default-thread run had two runtime timing failures; their limits remain unchanged.
- `cargo +stable run --locked -- selftest` with offline mode and a private temporary installation passed formats and staged telemetry capture; the board read was explicitly skipped.
- Every `tests/*.test.cjs` file passed when run individually with Node. Release-tag, exact-source release and all six README-shot checker cases passed. The Windows shell-check harness now prefers Git's tools, retaining the unrecorded-image refusal.
- The final browser fixture passed using the isolated Playwright dependency via `NODE_PATH=.taskq/browser/node_modules`; its screenshot matches `after.png` byte for byte.
- The canonical gate stopped because the task says `section 4` rather than literal `§4`. The rule-ID gate requires that ID in the task body or a trusted answer; it was not bypassed. Merge-result CI and live-host acceptance remain unverified (R12).
