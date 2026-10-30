# Project setup check

Owner: `taskq.md` § 6 (#1938). A new project must never stall silently: after `service install` or the tray's Add project, TaskQ checks the host and repository setup it needs and names each gap with its fix.

## Contract detail

`taskq.md` keeps the decision and links here; this text keeps the same authority (R8, R13).

### Command and triggers

- `taskq service check [--json]` checks the current project and exits 0 whatever it finds. A config that does not load is an error, as for every command, except that `publish` disagreeing with `delivery.preset` is the `delivery` item: the check then reads the project as the preset says.
- `service install` reads the project the same way and ends with the same check, so the tray's Add project, which runs `service install` in the picked folder, shows it too.
- Each service pass refreshes the local items and stores the result as `status.json` `setup`. The board's `pr-route` is read at most hourly; host labels come from the pass's own board read, so the check adds no other board request.
- `service status` prints each project's open gaps from its `status.json`; `taskq status` prints them after the R6 report when the local service's `status.json` is live. A missing `setup` prints nothing. Both show only gaps; unknowns stay for `service check`.

### Read-only

The check reads files, `host.json`, `taskq.json`, the runtime login probe (§ 5 `agents`), the § 8 Access report and the board's rules endpoint. It never writes a file, grant, label or board object and never runs a login. It never grants access (R9): each fix names an owner command or owner step.

### Items

Each item has an `id`, a `status` (`ok`, `gap` or `unknown`; unverified stays unknown, R12), one plain-language `text` and, unless ok, one `fix`: the exact command or the one owner step. A runtime is in use when its `host.json` limit is above 0 or missing, or when it is the default runtime that routes reviews (§ 3).

Host, per project:

| id | Gap when | Fix |
| --- | --- | --- |
| `checkout` | `<root>/.git` is missing | clone or `git init` the project at the root |
| `workspace` | `workspace: external` and the project has no `AGENTS.md`, which names the worker path (§ 7 rule 2) | add `AGENTS.md` with the workspace path |
| `cli <rt>` | the runtime CLI is not on the service PATH | install the CLI |
| `login <rt>` | the runtime CLI is not logged in | the relogin step (docs/relogin.md) |
| `access <rt>` | § 8 Access refuses the runtime, so its launches are held | `taskq service permissions --runtime <rt> --mode workspace --scope project` (Claude on Linux and Windows: `--mode full`) |
| `codex-dir` | Linux, Codex in use with a workspace grant, and `<root>/.codex` is missing | `mkdir <root>/.codex` |
| `host-label` | `claim: labelled` on this host and open tasks carry no `host-*` label | `taskq service capacity --host <host> --claim any`, or label the tasks |
| `token` | GitHub board and `<install>/token` is missing, empty or unreadable (§ 8) | the owner installs the token; never `gh auth login` |

Repository, per project:

| id | Gap when | Fix |
| --- | --- | --- |
| `delivery` | `publish` disagrees with `delivery.preset` (§ 7) | remove `publish` from `taskq.json` or set it to the preset's route |
| `pr-route` | `delivery.preset: pr` but the repository cannot merge by PR: no merge queue and no strict main protection, or the rules read is refused (HTTP 403: the plan of this private repository has no branch rules, so each PR needs a manual merge, § 7) | set `delivery.preset` to `direct`, or add a merge queue or strict main protection |

A `codex-dir` gap replaces the `access codex` gap it causes. A board without a rules read reports `pr-route` unknown.

### Output

Text: one line per item that is not ok, then its fix.

```
Gap: <text>
Next: <fix>
Unknown: <text>
```

No gap and no unknown prints `Ready: setup check passed`. `--json` prints `{"project": "<name>", "items": [{"id", "status", "text", "fix"}]}` with every item, ok ones included; `fix` is null when ok. `status.json` `setup` holds the same object plus `checked` (Unix seconds). Status lines are `Setup gap: <text>` and `Next: <fix>`.
