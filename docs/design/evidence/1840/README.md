# Loaded-project counters

Task [#1840](https://github.com/taskqorg/taskq/issues/1840), taskq.md § 4 (R8/R13).

The isolated browser fixture contains one loaded board with seven open tasks and one failed board with the cause `no access to the repository`.

- [Before](before.png): `All tasks ?` and the generic Partial advice.
- [After](after.png): `All tasks 7`, loaded state counts and one plain `Broken: no access to the repository` line.

`node tests/partial-project.browser.cjs` failed before the dashboard change (`All tasks ?`, expected `All tasks 7`) and passed after it. It also checks search-scoped totals, absence of the generic advice and unknown counts when only the failed board remains. `node tests/display-model.test.cjs` reproduced the same defect through the actual controller and passed after the fix.

These are Chromium headless screenshots of the bundled menu/dashboard with invented inputs. They do not establish native window behavior or live repository access. No production credential or service was changed.

The credential regression opens one board through the production provider registry and substitutes only its HTTP endpoint with a local fixture. The cached-token baseline failed because the old token's pause blocked the next read after replacement. With credential refresh enabled, `cargo test --locked --bin taskq next_read_uses_replaced_host_token` passed: the fixture observed both invented credentials in order, the old pause file remained, and deleting the token refused the next read.
