# #1703 native pass: Completed (A1) and Settings routes (A2)

- App: installed TaskQ.app on the Mac (tray `org.taskq.tray`); tray and service both report `4.6.40-dev.1792898459`, 8 projects, "No newer version reported" (screen 10).
- Time: 2026-10-25, about 15:15–15:40Z (22:15–22:40 Asia/Bangkok).
- Mode: read and close only. No operation was applied: no Send test, no Pause all, no grant, no Read current recovery, no Review updates, no Emergency stop.
- Network context: GitHub access from the Mac was unstable at about 15:23–15:33Z (curl: connection reset, HTTP/2 framing errors; probes alternated 200 and timeout). Hosts showed "stale 5–7m" and a warning badge in the same window.
- Screens: 10 JPEG files in this folder. Areas of the desktop outside the TaskQ windows are blacked out. AX notes come from the same session.

## A1: Completed

| Check | Result | Screen |
|---|---|---|
| First page loads real board rows (taskq, agentq, dashq, pilotq) with outcome and date | pass | 01 |
| Scroll paging loads older rows (reached 2026-10-13) | pass | not kept |
| Search filters the list (query "AgentQ") | pass | not kept |
| View editor: period + outcome (7 days + Not planned) gives the matching rows (2) | pass | 02 |
| Reset + Done returns the full view | pass, after a long wait | 03 |
| Slow load keeps rows and shows per-source progress or error | **fail** | 02, 03 |

**Finding A1-1.** After a filter change or Reset, the list drops its rows. It then shows only "Reading board history…" and "Load more" for more than 50 s. It shows no per-source progress and no error. The hosts were stale and GitHub was unstable at the time. The rows came back later without user action. Expected by A1: keep the rows that are on screen and show progress or an error per source.

## A2: Settings routes

The Settings list has 11 routes plus Emergency stop. Emergency stop was not opened.

| Route | Opens | Closes / Back | Result | Notes | Screen |
|---|---|---|---|---|---|
| Appearance | yes | Back to list | pass | 18 px, Summary, growth Up | not kept |
| Notifications | yes | Back to list | pass | OS allowed; per-event Off / Silent / Banner / Banner with sound; Send test buttons (not clicked) | not kept |
| Activity history | yes | Back to list | **check** | Hour window 21:40–22:40 shows ask 0, closed 0. The #1703 ask (14:55Z = 21:55 local) and the pilotq#42 release (15:04Z) fall in that window. Source scope not confirmed | 05 |
| Hosts and resources | yes | Back to list | pass | Per host Claude / Codex counts; the A40 fixture host shows "?" | not kept |
| Work mode | in another window | Back goes to Conversations | **fail** | Opens "Operation review" in the Workspace window, not in Settings. The Settings panel closes. Back does not return to Settings | 06 |
| Agents and access | yes | Back to list | pass, minor | Route title "Agents" and list label "Agents and access" differ. Shows the account e-mail (kept out of this package) | not kept |
| Projects | yes | Back to list | pass | Board counters, per-project intake rows | not kept |
| Delivery | yes | Back to list | pass | Per project: auto-merge for 7, direct for csgo; note that candidate approval and publication stay on the board | 07 |
| Services and support | yes | Back to list | pass, minor | Route title "Services" and list label differ. Host service running, PID and fresh observation; services for 7 projects; csgo appears only as "Incident reports: notice" | 08 |
| Recovery | in another window | Back goes to Conversations | **fail** | Same as Work mode: opens "Operation review" in the Workspace window, Settings closes | 09 |
| About and updates | yes | Close (X) | pass | Tray and service version, update state | 10 |

Settings list: screen 04.

## Findings

1. **A1-1 (fail):** Completed loses its rows during a slow reload and shows no per-source progress or error. Filed: [#1711](https://github.com/taskqorg/taskq/issues/1711).
2. **A2-1 (fail):** Work mode and Recovery leave Settings. They open "Operation review" in the Workspace window, the Settings panel closes, and Back goes to Conversations. Filed: [#1713](https://github.com/taskqorg/taskq/issues/1713).
3. **A2-2 (check):** Activity history Hour shows ask 0 and closed 0 for a window that contains at least one ask. Check which sources the counters read. Filed: [#1712](https://github.com/taskqorg/taskq/issues/1712).
4. **A2-3 (minor):** Route titles differ from list labels ("Agents" vs "Agents and access", "Services" vs "Services and support"). Filed with A2-1: [#1713](https://github.com/taskqorg/taskq/issues/1713).

## AX notes

- All routes have a visible Back control and a Close control; titles are text, not images.
- The Settings panel is a separate small window. Two routes (Work mode, Recovery) move focus to another window without a cue.
- During the stale period the only status signal in Completed is the text "Reading board history…"; it has no role or progress value per source.

## Not tested

Windows, light theme and alternate window geometry.
