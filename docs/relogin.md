# Runtime re-login runbook

The owner re-logs a runtime on a host through the PM session. The PM runs each step, passes the device code or link to the owner, and reads the result back. The owner only signs in in the browser. Never paste a token, code or key into a task, PR, chat log or board comment.

## 1. Find the logged-out runtime

On the host, from the project root:

```
taskq service status
```

A logged-out runtime shows `Login: <rt> ... not logged in` and `Next: <rt> admits no task; ...`. The heartbeat shows `login <rt> off`. Other runtimes keep working.

## 2. Choose the login, longest-lived first

Log in as the user that runs the taskq service (on Linux, the service user; on Windows, the Scheduled Task user).

### Claude

| Option | Lifetime | Use by taskq workers |
|---|---|---|
| `claude auth login` (claude.ai subscription) | refreshes itself while used | yes: the default |
| `claude setup-token` | long-lived token, about one year | not yet: the token works only through `CLAUDE_CODE_OAUTH_TOKEN` in the service environment, and taskq does not pass it |
| `claude auth login --console` | Console API billing | yes, but the owner chooses API billing |

Steps for `claude auth login`:

1. The PM opens a visible session on the host: on macOS and Windows, `taskq service setup --new` (#1426); on Linux over ssh, run the command in that ssh session.
2. Run `claude auth login`. It prints a URL. On a host without a browser, the PM gives the URL to the owner.
3. The owner signs in and, if asked, pastes the returned code into the session. The PM does not copy the code anywhere else.

### Codex

| Option | Lifetime | Use by taskq workers |
|---|---|---|
| `codex login --with-api-key` | no expiry until revoked | yes, but the owner chooses API billing |
| `codex login --device-auth` (ChatGPT plan) | refreshes itself; on Linux the refresh persists only with the #1424 fix deployed | yes: the default |

Steps for `codex login --device-auth`:

1. On Linux, check the #1424 fix is on the host first: the taskq release includes it, and the root-owned sandbox helper is replaced. Without it the refresh token rotates in a read-only `auth.json` and the login burns again (`refresh_token_reused`).
2. The PM runs `codex login --device-auth` in the host session. It prints a link and a one-time code.
3. The PM gives the link and code to the owner. The owner opens the link, signs in and enters the code. The code expires after a few minutes; on expiry, run the command again.

For an API key, the owner pipes the key on the host: `printenv OPENAI_API_KEY | codex login --with-api-key`. The key never goes through the PM chat.

## 3. Read back

Run `taskq agents` and `taskq service status` on the host. The runtime shows `logged in`, the account, the kind and, for Codex, a fresh `refreshed` time. Within 5 minutes the next probe clears `login <rt> off` from the heartbeat and the runtime admits tasks again. For Codex on Linux, check again after a day that `refreshed` moved and the runtime still reads `logged in`.

Evidence for a PR or task names the host, runtime, kind and the time of the readback. It never carries the account e-mail, a token, a code or a key.
