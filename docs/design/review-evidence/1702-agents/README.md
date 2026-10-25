# TaskQ #1702: Agents page evidence

Two `taskq tray --snapshot` WKWebView renders from the `taskq-1702` release build over `docs/shots/fixture.sh`. The fixture's `status.json` has two remote host rows added (`hermes-fra1`, `win`). Native screen capture was not allowed (TCC), so these are web-view snapshots, not screen captures. No sign-in, refresh, grant or setup was performed.

- `agents-remote-hosts.png`: remote hosts show heartbeat age only; client facts read unknown with the missing source.
- `agentq-unknown.png`: AgentQ in an unknown state shows its reason and Check again, with no setup prompt.
