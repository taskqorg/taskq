# Isolated native application checkpoint

Attempt `6537039713` continues the review requeue. **A2–A6 remain incomplete.** The tray-lock limitation is resolved for a disposable context; an explicit service-authority refusal now blocks the provider chain. [#2003](https://github.com/taskqorg/taskq/issues/2003) is filed and read back held, with no attempt (R3, R9, R12, §7.3).

## Candidate and isolation

[Manifest](manifest.json) pins checkout `d257507aed50788e831af01c1d4f7fafcb468399`, compiled revision `67f729e4dc35d20f044efe8cb7a3b6a939c5e048`, binary hashes, source bundle hashes and signing. Their application-source diff is empty. This is TaskQ 4.6.50, Cargo dev/unoptimized, not a prototype or a new release. Existing integration ancestors in [integration](../integration.md) are retained; later main reporting/backend changes are not claimed as tested here.

The unmodified product executable was copied into a temporary app bundle, signed with TaskQ Local Signing as `org.taskq.qualification1932`, and verified with `codesign --verify --strict` (exit 0). The separate identifier makes the disposable app addressable through CUA; it does not inherit or qualify production privacy grants. [Environment](environment.json) freshly confirms macOS 27.0.1/26A434, WebKit 22625.1.29.11.28, screen 1470×956 points, work area 1470×922, scale 2 and `.AppleSystemUIFont`. Actual application input used dark theme and 16/20 px. No OS theme or display setting changed.

`<fixture>` below denotes an owned OS-temporary directory; paths exported in evidence use that placeholder. The fixture is `docs/design/tray-fixture.sh` with eight extra projects, plus two disposable conversation records. Invented alpha/beta/gamma status files are incomplete observations, not live board state. The install has no production token, host policy or service owner record. Initial startup uses an empty project registry; invented projects are then attached as fixture input, without running installation or trusting folders.

```sh
fixture=$(mktemp -d)
mkdir -p "$fixture/install" "$fixture/tmp"
printf '%s\n' '4.6.50' > "$fixture/install/current"
: > "$fixture/install/projects"
TASKQ_INSTALL="$fixture/install" TMPDIR="$fixture/tmp" TASKQ_INCIDENT_REPORTS=off cargo run --locked -- tray --open
# The isolated signed bundle then runs the same `tray --open` entry point.
```

The first two empty-install launches exited 0 before writing a tray heartbeat because `current` was absent. Source inspection identified the existing current-version exit guard; pinning `current` made the native tray stay alive. The production lock file was never removed or modified. The first unbundled disposable process was stopped by exact recorded PID and exited 143; both subsequent bundled application runs exited 0 through their own Cmd+Q. No test service started.

## Native actions and observations

All gestures, clicks, keyboard actions and picker interaction used CUA against the isolated app. [Native draft AX](draft.ax.txt), [file/draft AX](draft-with-file.ax.txt), [Settings AX](settings-20.ax.txt) and [store readback](readback.json) attribute the observations.

| Requirement subset | Observation | Result and limit |
|---|---|---|
| S13/S14, Q-A2/Q-A8, C02/C03 | New chat; paste 40 Unicode lines in the real composer; inspect context | Draft persisted under its own ID; Send remained visible. Actual IME composition is Not tested. |
| S17/S18, Q-A7/Q-A11 | Save one disposable request; Check delivery | One pending setup record, same original ID; no job or native session. No second send occurred. This is local request reconciliation, not provider delivery. |
| S18 | Cancel queued launch | Native view reports cancellation before dispatch; record becomes `cancelled`, job/session stay null. The sidebar still says Delivery unknown; no runtime work is inferred. |
| S13/S23, C03 | Create a second independent draft; Unicode paste and Shift+Return | Three lines retained; no dispatch. Uppercase `Shift` was rejected by the CUA key parser; lowercase `shift+Return` worked. This was a tool correction, not an application defect. |
| S23 | Detach → Return to main; Archive → Undo archive | Same unsent draft text and ID retained. No setup request added. Permanent delete and every archived-task policy remain Not tested. |
| S14/S23, C09 | Native file picker selects an explicit disposable 55-byte text file | Real file chip appears with exact name/size. No attachment sent. Drag/drop, preview, removal/refusal and provider delivery remain Not tested. |
| S24, C03 | Cmd+Q; relaunch isolated client; reopen Conversations | Text, file metadata, archived flag and caret equal the saved pre-restart values; native view shows the same draft and file. Setup record count stays one, cancelled. Pending-send/anchor/service recovery remains Not tested. |
| S05/S22, M25 | Settings home → Appearance; 16 → 20 px; Summary; native wheel down/up | Settings remains readable; lower controls reachable; header/footer stable in the inspected frames. Full routes/theme/scale matrix remains Not tested. |
| Q-A9/S22 | Two CUA scrollbar drags; keyboard-resize attempt | No visible movement/resize established. These checks are Not tested, not passes or diagnosed application defects. Further identical attempts were stopped. |

One inspection click used a stale AX index after a silent full-tree evidence capture and queued the first disposable draft. This was an automation-targeting error, not spontaneous application dispatch. Check delivery and cancellation used freshly observed controls; [readback](readback.json) proves no native job/session and no second request. File-picker path selection was likewise verified before Open; an initial clipboard attempt did not select the intended file and was replaced by the visible Go to Folder field. No unrelated file was opened or attached.

## Exact blocker and privacy limit

The attempted isolated command, with its own install and TMPDIR, was:

```sh
TASKQ_INSTALL="$fixture/install" TMPDIR="$fixture/tmp" TASKQ_INCIDENT_REPORTS=off cargo run --quiet --locked -- service run
```

It exited 1: **`Error: a worker cannot run the host service`**. [Exact refusal](service-refusal.txt) retains the timestamp and command diagnostic. `service status --json` separately reports `state: unknown`, `fresh: false`, reason `host service owner record missing`. After client restart, Send is disabled with `Claude Code owner-session readiness is unknown; refresh or inspect Agents in the tray.` Unsetting worker identity, installing a host service, manufacturing grants or launching providers outside the service would bypass R3/R9 and is not a remedy. #2003 requests owner-run preparation, not an implementation change.

WebKit also logged [storage creation denials](webkit-storage-denial.txt) with Cocoa 513/POSIX 1 for the disposable app's WebsiteData directories. No privacy prompt was accepted or grant changed. Whether the denial is TCC or worker sandbox enforcement is unknown; production grant health is not inferred. This limit is reported in Open and is not a TCC owner ask.

## Acceptance and review set

A1 remains pinned for this candidate; A7 records explicit gaps. A2/A4 gained native subsets above. A3 has no completed provider flow and is blocked by #2003. A5 measurements remain the earlier bounded instrument samples; no new performance target or full I/O/cache result is claimed. A6 has a real Summary preference selection, but actual OS-slot ink/overflow/pause/A-mixture acceptance remains Not tested. Physical input/IME, VoiceOver, other themes/displays and the complete preservation ledger remain open in #1932.

The current ten-screen selection is the eight entries 01/02/04/06/07/08/09/10 from [the earlier set](../resumed.md#review-set), plus [native draft/context](draft.png) and [native Appearance 20 px](settings-20.png). Earlier 03/05 are historical, no longer selected. The two new images were opened with the image-viewing tool: context/Send and large-text controls are visible in the frames; a still does not establish whole-history access, scroll anchoring or motion. The existing short renderer clips remain explicitly sampled WKWebView recordings, not OS-slot or physical-input evidence.

Production services, grants, saved views, real task answers and owner sessions were kept unchanged. The only live-board addition is held blocker #2003; it was not started. No implementation fix or rule amendment is included (R13).

[Selected canonical gate](checks.md): sandbox-blocked, not PASS; 752 unit and 21 integration passes, one sandbox-apply denial, 96.178 s / 900 s, no retry.
