# Conversations native baseline checkpoint

Task: https://github.com/taskqorg/taskq/issues/2159

Baseline source: `ba7652694e5ab3aa343ad7f1767d04b0f7c50aa8`.
Observed installed UI: `v4.6.59-dev.1793573522`; its exact source identity has not been verified. No implementation changes, installation, restart, production fault injection or PM message submission occurred.

## Observations before edits

- The single-centre native Conversations window is present. The sidebar filter wraps `In progress` and its count onto separate lines at the existing window/font settings. The captured [native sidebar](before-sidebar.png) retains that density defect.
- Back from Conversations reached an existing Operation review. Its page displayed `Not saved: Unsupported host capacity payload; refresh and review current settings`. Back then returned to Settings / Balancing, and another Back returned to Settings home. That sequence did not reproduce a failed Back. No capacity edit was submitted.
- Closing Settings and invoking Command-R returned a native Conversations reader. Its selected source differed from the initial observed request, but intervening navigation and saved state prevent attributing that difference to Reload. Draft, anchor and no-replay recovery are not qualified by this observation.
- A separate empty native draft named `2159 PM usability checkpoint` is selected for project `taskq`, request `fc4d2c4ab05d1ab95b085df8590301b1`. The UI shows `Claude Code · mac`, no runtime selector and a disabled Send. The draft was not sent. [Native state](before-pm-runtime.txt) with its screenshot retained privately preserve the observation.

## Runtime boundary before implementation

A5 requires launching Codex through the TaskQ GUI. `docs/design/ai-workspace.md` Contract detail currently names the qualified Claude Code runtime and says the any-runtime target remains unqualified; its earlier scope explicitly accepts a Claude-only handoff. `src/tray.rs::owner_runtime` resolves only `runtime::OWNER`, which `src/runtime.rs` fixes to Claude. The accepted TW4 target allows a named runtime choice, but is marked unshipped. Steer 6565264499 explicitly authorizes fixing this within A5 and reconciling canonical rules under R13, without grants or capacity changes.

The authorized resolution is explicit per-request runtime selection under the existing owner-session authority, with no global provider, capacity or grant change. No implementation has been applied at this checkpoint.

Subsequent steers 6565267903 and 6565278229 establish concurrent use of the shared native UI by the owner PM. The unattributed route/selection changes above cannot qualify a worker-only reproduction. Production navigation stopped after saving the test draft; further native acceptance needs an explicit window handoff. The owner PM retains draft `0bbf213aace0afa5c7d26e6781b755f9` and owns the single Codex PM launch. The worker continues with isolated fixtures.

## Remaining evidence

A1 remains partial: the sidebar defect is reproduced; original Back/window failure and full native hierarchy review remain open. A2-A8 are incomplete. Prior #2140 performance failures and untested native boundaries remain failures/unknowns under R12. The unattributed unsupported-workspace-action log is not used as a cause.
