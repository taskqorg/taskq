# Claude window publication regression (#2393)

## Native finding

The [sanitized Windows response](windows-oauth-usage.json) came from one read-only OAuth usage request on the service route. It retains quota fields only; credentials, request headers, cookies and account identity were never exported. The credential file was byte-identical before and after the request. No token refresh or retry ran. The first observation was isolated. The subsequent [native cadence trace](windows-oauth-cadence.json) records two requests using the service's curl configuration and 120-second period, without retries or token refresh; both credential readbacks were unchanged. It confirms the null surface and readable weekly/Fable fields on that route at the service cadence, without installing a candidate service.

The response contains weekly utilization 43% and scoped Fable 4%. Its Fable scope includes `surface: null`. The previous parser rejects every scope key except `model`, including that null dimension, so it discards the whole poll. `seven_day_breakdown`, kind, severity and activity are explanatory metadata, independent of a quota reading.

The [released heartbeat snapshot](released-heartbeats-before.json) records win 4.6.78 and mac 4.6.79-261104-13 both publishing `unknown usage shape changed`. Those are before observations, not candidate qualification.

## Authorized behavior

[Task #2393](https://github.com/taskqorg/taskq/issues/2393), answer 6643757786, authorizes publishing readable windows alongside explicit unknowns for unreadable windows. R12 keeps utilization, scope and account/window identity unknown when unsupported. Aggregate headroom remains unknown for an incomplete account. Transport/authentication/full-response failures retain whole-account unknown. Polls never change login files or quota holds.

## Verification and limits

The [before check](before-regression.txt) fails on the unchanged parser with `usage shape changed`. The [corrected usage checks](after-regression.txt) pass: 20 passed, one manual native probe ignored. They cover the native 43%/4% response through parsing, persistence and heartbeat serialization; mixed valid/invalid windows; structured replacement of legacy readings; null versus non-null unsupported scopes; missing/negative utilization; malformed lists and full responses; safe diagnostics; account/fleet identity and incomplete headroom; failure projection and the existing no-login-write/no-quota-hold-release guards.

The two new store/aggregation tests initially used the shared fixture time after the captured response's reset; that correctly yielded unknown headroom. They now use the native response's observation time. A development run also inherited worker bindings and hit an owner-only account-command fixture; the passing run uses the checker's isolated child environment. Neither issue was resolved by weakening expiry or permission assertions.

`python scripts/check.py --lint` passed before source checks, with the existing root contract's over-70% `COMPACT` warnings. The canonical selected gate runs on the rebased commit before publication; its result is reported on the task. Local tests do not qualify native Mac behavior or merge-result CI.

The first canonical gate passed on `31910f17fd82a7a145a39106d1671d9b4e04be97` in 536.297 seconds: 1,028 unit tests, 21 integration tests, offline selftest, Node components and release-script checks. An independent read-only review found a missing recovery case despite that pass: a subsequent failed poll cleared incomplete evidence and exposed cached partial headroom as 57%. The new recovery regression fails before correction and passes after it. The corrected store preserves the same account's unreadable windows with their original observation time across failures, publishes their still-fresh unknowns for fleet readers, and does not renew their expiry. The review found no further actionable acceptance defect in that correction. A second canonical gate is necessary because the candidate changed; its final result is reported on the task.

A2's released outcome and A4 require integration, the normal PM-triggered release (taskq.md ?8.1), and fresh win/mac heartbeat readback. This worker does not change channels, logins, quota settings or the ccusage publisher. Source tests and a generated heartbeat are not released-service proof. Qualification against ccusage remains in DashQ #255.
