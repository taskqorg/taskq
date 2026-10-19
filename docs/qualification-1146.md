# Historical reliability evidence (#1146)

This read-only review establishes a completed observation window, but does not
establish complete incident coverage or the historical closed-heartbeat lease
qualification. Missing observations remain unknown (R12). No services, grants,
models or runtime fixtures were changed or started for this investigation.

## Release and observation window (A1)

PR [#1144](https://github.com/taskqorg/taskq/pull/1144) merged at
2026-10-14T21:22:04Z as `16d861ebe3941e1cf00c89555a62d85708f72307`.
The first containing published release is
[v4.2.0](https://github.com/taskqorg/taskq/releases/tag/v4.2.0), published
2026-10-15T00:57:21Z, tag commit
`f33df40dedd219678b02195c264f2367ac66e96e`.
`git merge-base --is-ancestor 16d861ebe3941e1cf00c89555a62d85708f72307 v4.2.0`
returned 0; `git tag --contains` starts at v4.2.0 in version order.

The retained [rollout receipt](https://github.com/taskqorg/taskq/issues/1053#issuecomment-6182023624)
was posted at 2026-10-15T01:07:57Z. It records Mac heartbeat #722 and Windows
heartbeat #745 at v4.2.0, plus release run 38662361172. This is a recorded
operator observation, not a fresh remote-host qualification.
Use the elapsed interval **[2026-10-15T01:07:57Z, 2026-10-16T01:07:57Z)**.
No new 24-hour wait is needed. The receipt is a timestamped board comment;
its original contents were not independently verified against an immutable
archive of heartbeat edits.

## Incidents and recurrence (A2)

On 2026-10-19, the all-state GitHub issue search for this exact interval and
`"taskq incident" in:title` returned one issue:
[#1262](https://github.com/taskqorg/taskq/issues/1262), opened
2026-10-15T10:10:12Z. Its payload names `update.smoke`, release 4.4.2,
revision `875b744019dbf985e20f1eb0e16a12612edae976`, macOS, and `smoke:failed`.
It is an update smoke failure, not evidence of the heartbeat conflict detector
recurring. No double_attempt report was found by that query. Search is not a
complete issue enumeration or proof of uninterrupted reporting on both hosts:
renamed/deleted reports, reporting failures, opt-out and host downtime remain
unqualified. Therefore the count of actual duplicate-attempt incidents is unknown.

The two observations already on [#1146](https://github.com/taskqorg/taskq/issues/1146)
are before the selected window: #1147 reviewer spawns at 2026-10-14T23:33:19Z
and 23:33:26Z; #1181 reviewer spawns around 2026-10-15T00:44Z.
Two spawn comments alone do not establish two native children (section 3).
Their launch/exit receipts were not established here; neither is dismissed as a
false alarm. PR #1144 changed detection of newer losing leases, not fold/claim.
It cannot itself prove those races safe.

## Heartbeat ownership and native receipts (A3)

[#1131](https://github.com/taskqorg/taskq/issues/1131#issuecomment-6177471608)
records candidate `78aff10ab872bc9d380f37fdd68e457f652d32b8` and explicitly
leaves A3 live readback open. The
[owner answer](https://github.com/taskqorg/taskq/issues/1131#issuecomment-6177649652)
requires post-release proof; it does not waive it.

PR #1144 retains the historical #1131 reviewer claim race: accepted Windows
spawn 6177497818 at 2026-10-14T20:17:38Z; losing Mac spawn 6177498100 one
second later; Windows heartbeat at 20:17:40Z listed the winner. Its report says
the loser was absent from heartbeat edit history and explicitly limits Mac
evidence to that history, rather than independently read native service logs.
This supports recorded claim ownership, not native exit or closed-state proof.

Fresh board reads on 2026-10-19 returned **OPEN**, `closedAt: null`, for both
#722 and #745. Mac #722 listed this assigned attempt `#1146 6257399333`;
Windows #745 listed focus off and no attempt. This establishes present Mac
lease visibility, not visibility while closed or historical Windows liveness.
The first 100 issue events on each returned no closed/reopened event; those
unpaginated reads cannot establish absence across the full history.
No matching T1131/R1131 native logs were found in the project log inventory.
The later `released ... exit` comment on #1131 is not a native process receipt.
Historical closed-state, native termination and expired-lease-to-gone evidence
remain unknown. No new attempt was manufactured to fill these gaps.

## Review boundary (A4)

This report changes no product decision (R13). It separates board observations,
retained operator statements and missing process proof (R1, R12, sections 3–4).
No executable code or tests changed; documentation checks suffice under Testing.
Existing PR #1144 before/after detector regression evidence is retained, not
claimed as rerun evidence. Review must check source references and these limits.

Recommended owner decision: accept this bounded historical report with A2
coverage and A3 native/closed-state gaps explicitly unknown, then submit it for
review. Alternatively retain the original proof requirement and park until an
owner supplies retained heartbeat history and native attempt/exit receipts.
Neither option authorizes service changes or new cross-host calls.
