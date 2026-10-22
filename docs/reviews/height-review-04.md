# Height-based column review 04

Owner rejected heat intensity because it did not communicate dynamics and explicitly requested quantity by column height. Palette A, six bins and closed-only volume are retained. Owner selected A. Final geometry and state cues were subsequently approved in review 05, recorded in [visual-decisions.md](visual-decisions.md). This is the historical style comparison and source-proof page; the approved surface references supersede its incidental coordinates.

## Compared candidates

- **A, recommended:** six separated solid green columns, constant color, height = closed / maximum closed across the six bins. Zero baseline; one shared linear scale.
- **B:** identical values, scale and green fills inside six fixed-height cell outlines. This keeps empty slots visible but adds visual clutter. The outlines mean the window maximum, not a task target or completion percentage.
- Ask and blocked stay in exact History details. No failed events are fabricated. Failed markers, partial/unknown bins, pause and overflow remain unresolved.
- Provisional 22 pt geometry: current counter numerals at a 9 pt baseline; graph x = 1 + 3.5 × bin index, column width 2.5 pt, bottom 21 pt, maximum height 9 pt, top 12 pt. Scale the same geometry for enlarged and small samples. Cell B adds a 0.35 pt neutral outline. Counter ink calibration and native pixel snapping are not qualified.

[Dark PNG](img/height-review-04-dark.svg.png) · [light PNG](img/height-review-04-light.svg.png). Editable SVGs share the basename. Two sheets, two candidates each; four panels total. These are research artifacts, not native tray captures.

## Verified TaskQ source

Frozen [fixture](real-data-2026-10-22.json): 2026-10-22 21:16:25 UTC+7. Use TaskQ alone for an understandable comparison, without changing the approved all-board icon scope.

- Source root: `<projects>/taskq`; source board: `https://github.com/taskqorg/taskq/issues`.
- Frozen and reread `events` SHA-256 are identical: `7a8a84104cc6e41db07d02a86c4a01be063877cfd9a3e58c0d5505f56eb486d8`. All **72** cached close/ask/blocked window records remain present; their 39 close records produce the columns below.
- GitHub connector independently confirms [#1586](https://github.com/taskqorg/taskq/issues/1586), “Draw history bars at least 4 px wide by summing bins”, is closed, `closed_at = 2026-10-22T13:28:31Z`; cache close event at 20:28:39 UTC+7.
- GitHub connector independently confirms [#1588](https://github.com/taskqorg/taskq/issues/1588), “Do not report Run state unknown when a native history only changes mtime”, is closed, `closed_at = 2026-10-22T13:58:19Z`; cache close event at 20:58:25 UTC+7.
- Cache event time and GitHub `closed_at` are distinct timestamps (8 and 6 seconds apart in these examples). The six-bin fixture uses cache event times, matching the local history reader. Two cross-checks are corroboration, not a claim that every issue was freshly fetched.

| Interval, UTC+7 | Closed | Source TaskQ issues |
|---|---:|---|
| 21 Oct 23:00–22 Oct 03:00 | 5 | [#1507](https://github.com/taskqorg/taskq/issues/1507), [#1506](https://github.com/taskqorg/taskq/issues/1506), [#1465](https://github.com/taskqorg/taskq/issues/1465), [#1514](https://github.com/taskqorg/taskq/issues/1514), [#1499](https://github.com/taskqorg/taskq/issues/1499) |
| 22 Oct 03:00–22 Oct 07:00 | 4 | [#1517](https://github.com/taskqorg/taskq/issues/1517), [#1519](https://github.com/taskqorg/taskq/issues/1519), [#1466](https://github.com/taskqorg/taskq/issues/1466), [#1521](https://github.com/taskqorg/taskq/issues/1521) |
| 22 Oct 07:00–22 Oct 11:00 | 7 | [#1467](https://github.com/taskqorg/taskq/issues/1467), [#1531](https://github.com/taskqorg/taskq/issues/1531), [#1420](https://github.com/taskqorg/taskq/issues/1420), [#1470](https://github.com/taskqorg/taskq/issues/1470), [#1535](https://github.com/taskqorg/taskq/issues/1535), [#1522](https://github.com/taskqorg/taskq/issues/1522), [#1523](https://github.com/taskqorg/taskq/issues/1523) |
| 22 Oct 11:00–22 Oct 15:00 | 12 | [#1524](https://github.com/taskqorg/taskq/issues/1524), [#1541](https://github.com/taskqorg/taskq/issues/1541), [#1533](https://github.com/taskqorg/taskq/issues/1533), [#1526](https://github.com/taskqorg/taskq/issues/1526), [#1527](https://github.com/taskqorg/taskq/issues/1527), [#1391](https://github.com/taskqorg/taskq/issues/1391), [#1409](https://github.com/taskqorg/taskq/issues/1409), [#1546](https://github.com/taskqorg/taskq/issues/1546), [#1437](https://github.com/taskqorg/taskq/issues/1437), [#1549](https://github.com/taskqorg/taskq/issues/1549), [#1548](https://github.com/taskqorg/taskq/issues/1548), [#1550](https://github.com/taskqorg/taskq/issues/1550) |
| 22 Oct 15:00–22 Oct 19:00 | 7 | [#1555](https://github.com/taskqorg/taskq/issues/1555), [#1557](https://github.com/taskqorg/taskq/issues/1557), [#1561](https://github.com/taskqorg/taskq/issues/1561), [#1558](https://github.com/taskqorg/taskq/issues/1558), [#1564](https://github.com/taskqorg/taskq/issues/1564), [#1567](https://github.com/taskqorg/taskq/issues/1567), [#1562](https://github.com/taskqorg/taskq/issues/1562) |
| 22 Oct 19:00–22 Oct 23:00 | 4 | [#1565](https://github.com/taskqorg/taskq/issues/1565), [#1572](https://github.com/taskqorg/taskq/issues/1572), [#1586](https://github.com/taskqorg/taskq/issues/1586), [#1588](https://github.com/taskqorg/taskq/issues/1588) |

The last interval is ongoing: 4 recorded closes cover 19:00–21:16:25 so far. Do not infer a slowdown from comparison with complete four-hour bins. Closed events are completed task movements, not sampled concurrent tasks or a completion percentage. Cached observations do not prove uninterrupted collection.

## Inspection

- Pass: both final PNGs opened and visually inspected; all six columns, numeric labels, baseline, time labels, small samples and candidate boundaries are present without clipping.
- Pass: heights are linear from zero; 4 is one-third of 12, equal 7 counts have equal heights. Constant green removes the previous opacity ambiguity.
- Observation: A makes the peak and equal-height pairs clearer; B preserves the rectangular cells but adds competing contours, especially in the small samples. Owner selected A; final surface review 05 is approved.
- Not tested: actual menu bars, notch hardware, Dock, menu interaction, zero, failed > 0, unknown/partial cues, multi-digit counters and pause.
- No source, canonical design specification, service, installation or board data changed. Commit and handoff status are recorded by the final delivery; this page supplies the historical source proof.
