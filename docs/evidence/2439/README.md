# Accepted reports collector checkpoint

Before behavior changes (R1, R6, R12; task #2439 A1/A2): the read-only
`taskq status --project dashq --json` projection still lists #247 in Decide.
The retained [checkpoint](checkpoint.json) identifies the observed board time and
result SHA. GitHub comment [6665799047](https://github.com/taskqorg/dashq/issues/247#issuecomment-6665799047)
contains `taskq: report-accept 3f245b810c52a6d140b118219d16e395115e4c87`,
with MEMBER association and identical created/updated timestamps
`2026-11-05T18:22:49Z`. The issue update reads `2026-11-05T18:22:50Z`.
No decision was replayed. The checkpoint is more than ten minutes after acceptance.

## Recurrence

#2087 overlaps only the all-issues scan against five-minute list lag. #2091
requires a comment read more than five minutes after the issue update before
reusing the row. Both protections remain necessary. Neither protects discovery
through the frequent task-labelled scan: its cursor follows the newest observed
task minus 30 seconds, without the list-lag overlap. A newer task can move that
cursor past an omitted accept update; the comment-settling guard never sees it.
#2352 extended the all-issues fallback from five to thirty minutes and changed
collection scheduling to every three polls (capped at half ttl). Its existing
regression waits a whole full-scan interval, so it cannot detect this delay.

The deterministic reproduction uses the same accept/SHA relationship, a settled
old row, an omitted decision update and a newer listed task. It checks Results
membership and Decide, both continuously and from a retained cache after restart.
This establishes the code defect; the historical host's exact HTTP replies are
unavailable, so their precise ordering remains unknown (R12).

## Verification

Checkpoint commit: `4ee0fff1` (behavior unchanged).

`cargo test --locked --bin taskq incremental_lag_reconciles -- --nocapture`
failed before the fix: `accepted report stranded by incremental cursor (restart=false)`.
The initial attempt could not build without esbuild; `npm ci` supplied the locked
build dependencies before this failure was observed.

The fix overlaps the existing task-labelled read by `LIST_LAG + 1`, preserving
full-scan cadence and conditional-request URL reuse when the board is unchanged.
A disposable `incremental_overlap` version marker makes older caches rebuild
once: otherwise a missed decision older than the retained verified time minus
five minutes could remain undiscoverable even after restart. No decision write,
host policy, lifecycle rule or existing guarding assertion changes.

Refresh interpretation: reports.md requires incremental collection on ordinary
passes, and the collector schedules every `min(3 * poll, ttl / 2)` seconds.
Under the existing five-minute board-lag assumption, discovery now takes the
first completed due collection after list/comment lag clears, rather than the
thirty-minute full scan. Service-pass scheduling and board-read duration still
apply; stalled or failed reads remain unknown. The regression removes the accept
at 362 simulated seconds, before the full scan, including restart. It separately
checks an old cache already 601 seconds past the missed decision and verifies
that repair writes no comments. This is collector/model evidence, not an installed
service qualification (R12).

Final checks are recorded in the task result. The reports module retains its
SHA/trust/edit, pagination, late-comment, missing-source and idempotent-write
protections; no existing assertions were weakened.
