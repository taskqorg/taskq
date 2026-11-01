# Antigravity runtime evidence · #2136

The candidate adds Antigravity CLI workers/reviewers and the owner-approved R13, §4, §8 and §9 policy. It does not change live grants, capacity, accounts, permissions, models or host settings.

## Adapter and admission checks

Filtered Rust tests cover Antigravity spawn/resume arguments, literal JSON input, conversation fencing, terminal events, quota unknowns, worker/reviewer admission and preserved attempt ownership. Golden admission traces cover the third subscription, its strict pin, disabled capacity and default-runtime fallback. Existing setup and status golden checks retain their assertions.

Before/after evidence: third-runtime fallback initially failed; it passes after ordered selection. The unchanged legacy prefer-codex golden initially failed, then passed after retaining its strict worker pin. Setup initially demanded unused alternatives; the existing check passes after limiting prerequisites to selected/default, enabled or explicitly pinned runtimes. The status golden differed only by the newly supported runtime name.

Compact owner section citations initially failed the rule gate. Parser regressions now pass while retaining ID, subsection, trust, attempt, revision and selected-option boundaries. A planted Antigravity name outside its adapter also fails the architecture check.

## Browser inspection

Instrument: `PANEL_EVIDENCE=docs/design/evidence/2136 node tests/tray-panels.browser.cjs`. These are illustrative browser fixtures, not installed/native TaskQ or Antigravity sessions. All four final captures were opened and inspected. The Antigravity examples use 320 px width and 20 px type; names and counts remain visible without horizontal overflow.

- [Home with Antigravity](2136-home-antigravity.png): named total, runtime split and neutral terminal control opening the exact task-agent view. That control was initially absent; component and browser regressions fail before and pass after the fix. No native session URL is invented.
- [Agents with Antigravity](2136-agents-antigravity.png): version, sign-in, account and access uncertainty remain visible.
- [Existing Home](G06-home.png) and [project](G05-project.png): existing Codex rows, wrapping and independent navigation remain visible.

## Qualification gaps

Native Antigravity subscription execution, actual reviewer continuation, authentication, transcript access, supported versions and each OS/permission/lifecycle boundary remain unqualified (R12). macOS bounded access needs its own operator grant; unqualified containment refuses, with no Full fallback. The protected app-server host fixtures encounter sandbox `Operation not permitted` when writing their invented Codex configuration; this is sandbox-blocked, not PASS. Merge publication still requires green merge-group CI.
