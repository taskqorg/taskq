# Issue command

## Contract detail

`taskq issue` keeps its decision in `taskq.md` § 5 and its detail here, under the R8 size bound; this text has the same authority (R8, R13).

- Scope: any issue of the configured board, task or not.
- Read: `taskq issue N` prints JSON `{number, url, title, state, labels, body, comments}`; each comment is `{id, author, created, edited, trusted, body}`; `url` is the canonical issue URL.
- Write: `taskq issue N --comment T|--label L [--op-id ID] [--json]` adds one comment or one label and reads the effect back. `--json` prints the receipt `{issue, url, op_id, outcome, comment|label, reason, next}`; `outcome` is `accepted`, `rejected` or `unknown`.
- Op id: the comment's last line is `op-id: <id>`. A repeat returns the first comment; a repeat with changed text is refused. A label already present is accepted without a write. An `unknown` outcome is retried with the same op id.
- Refused: a comment whose first line starts with `taskq:`, and the labels `taskq`, `taskq-host` and `q-*`; task commands own them.
