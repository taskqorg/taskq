# Mac model-window observation

Candidate: `1f0d54aa865eb531530ee3dfc155888a96a27cc9` plus the local Claude metadata and quota User-Agent fixes. Mac arm64, Rust/Cargo 1.98.1, warm isolated build cache.

## Claude A1

The real `https://api.anthropic.com/api/oauth/usage` returned HTTP 200 on 2026-11-04 at 17:48:28 UTC. [Sanitized response](macos-oauth-usage.json): general weekly utilization 38%, model-scoped Fable 4%, both with reset times. No login, cookie, account identity or token is retained.

The response also contains `seven_day_breakdown`, an explanatory object with `as_of`, `window_started_at` and `rows`, without `utilization`. Its contents are omitted. The original candidate treated every non-null `seven_day*` field as a quota window and rejected this real response as `usage shape changed` before reading Fable.

The existing `model_windows_claude_scoped_limits` test now consumes this native projection. It failed before the fix with `native OAuth metadata must not hide Fable: "usage shape changed"`. The fix excludes only the known metadata key; an additional assertion retains rejection of an unreadable `seven_day_fable` window. Existing structured-window precedence and malformed-scope protections remain.

The first native read returned 200. An initial evidence projection assumed every prefixed field had `utilization` and failed on the metadata. A subsequent read returned 429; further Claude reads stopped until the service's own regular poll recovered. The final read above returned 200. These observations do not prove the cause of provider throttling.

## Antigravity A2

The same Mac's existing default login returned HTTP 200 from `loadCodeAssist`; `currentTier.usesGcpTos` was absent, so the candidate-selected consumer route was `daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`.

At 17:46:53 UTC this route returned only `3p-weekly` and `3p-5h`, both with remaining fraction 1 and reset times. [Original generic-agent response](macos-antigravity-usage.json).

The working ccusage source is `alexkirs/ccusage-mac` at `f900407645d329d9a82e8f540084109bab31a6ed`, `gemini.lua`. It uses the same quota endpoint, with `antigravity/hub/2.28.2 darwin/arm64` as User-Agent. Its already-loaded cache contains Gemini weekly 74%, Gemini 5h 0% and both 3p windows 0%. The native agy Google userinfo request returned 200 and matched the ccusage login's email and Google subject; provider-returned project also matched ccusage's cached project. Only boolean comparisons are retained, not the identities.

Production/daily `loadCodeAssist` and the richer ccusage metadata still returned only 3p quotas with the generic User-Agent. Changing only that header, with the same agy login, project and quota route, returned all four windows. The minimal portable `antigravity/hub/2.28.2` header also returned them, without an invented OS suffix. [Native response](macos-antigravity-compatible-agent.json). No ccusage access token was used by the TaskQ-route observations.

At 18:06:50 UTC, an independent request equivalent to the corrected candidate's full load-to-quota route matched the already-loaded ccusage cache across all four windows: 0 percentage-point difference and at most 5 seconds reset difference; the cache was 5 seconds old. [Comparison](macos-antigravity-comparison.json). This qualifies that native Mac producer observation, not an installed released-service heartbeat, other hosts or the public publisher cutover.

The existing poll-to-heartbeat chain now guards the compatible header and failed before the fix. The bucket parser also consumes the sanitized native response, retaining all four windows, 74% Gemini weekly and their resets. Malformed-window, unknown, privacy and hold protections remain.

## Boundaries and remaining proof

Native reads used the existing read-only credentials and the candidate's provider routes. No login refresh, account switch, security setting, service installation, ccusage publisher, queue state or PR approval was changed.

The existing focused usage suite passed on the original candidate: 16 passed, 1 native instrument ignored. The first build lacked `esbuild`; installing the existing package lock with lifecycle scripts disabled resolved setup. No new dependency was added.

After both fixes, `cargo test --locked --offline --bin taskq runtime::usage::tests -- --test-threads=1` passed: 16 passed, 1 native instrument ignored (3.87s execution). The quota-header chain failed before the A2 fix with "the generic agent hides Gemini quotas on the native route" and passed afterward. The final `python3 scripts/check.py --base 026db2524d6d5d7b34452d4c8d0fbf17073df7b9` gate passed on Mac in 164.888s / 900s without retries: 1014 Rust unit tests and 24 integration tests passed, with offline selftest, Node and release fixtures. Contract compaction warnings remain and are already owned by #2377; no duplicate task was created.

These are not released-service heartbeat proofs. At the start of the Mac work, A3 still required released proof in issue #2359 despite the two historical owner answers deferring that comparison to DashQ #255. Align that acceptance once before resuming the queue; do not repeat the owner question or resume the Windows attempt while the Mac result is being prepared.

## Integration plan

1. Re-read issue #2359 and PR #2372. Apply the prepared patch to the existing `taskq-2359` candidate, preserving the original producer changes; do not open a duplicate task or parallel worker.
2. Align A3 with the recorded owner choice, subject to the current owner's confirmation if needed. Proposed wording: "Record native producer evidence and regression checks here. Released TaskQ heartbeat-to-ccusage completeness, percentages within 5 points and resets within 600 seconds are qualified in DashQ #255 before publisher cutover. Preserve logins and keep ccusage pushing."
3. Publish the reviewed candidate only with current authorization, then use the existing review/merge-group route. No PR approval or merge is implied by these local checks.
4. After the normal release, run DashQ #255's live comparison against the still-running ccusage publisher. A clean native Mac comparison does not satisfy this released-service requirement. Keep other-host and public-page proof explicit.
