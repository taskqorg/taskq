# Tray Held counter and Start (#1476)

`mac-held-dark-project-16.png`, `mac-held-light-project-16.png`: native WKWebView (`taskq tray --snapshot`) of the
storefront project from `docs/shots/fixture.sh`: the strip shows the Held capsule last and no Closed capsule; the Held
group lists draft #42 with its Start button.

`browser-held-drafts.png`: headless Chromium, not a native WebView. Held selected in the project strip with two drafts:
one Start per draft and `Start 2 held` for the shown group (`tests/tray-state-filter.browser.cjs`).

Reproduce on macOS from the repository root:

```sh
T=$(mktemp -d); TASKQ_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1) sh docs/shots/fixture.sh "$T"
printf '%s\n' "$T/storefront" "$T/billing-api" > "$T/install/projects"
TASKQ_INSTALL=$T/install target/debug/taskq tray --snapshot "$T/shots" --route project
FILTER_EVIDENCE=$T/filter node tests/tray-state-filter.browser.cjs
```

WebView2 (Windows) evidence: not captured; this worker has no Windows host.
