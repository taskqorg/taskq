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

Pending before/after run. No owner rule or existing guarding assertion changes.
