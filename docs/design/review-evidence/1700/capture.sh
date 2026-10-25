#!/bin/sh
# #1700 A5: native View window captures (WKWebView snapshot of the `views` surface at its 360x600 default size)
# over docs/shots/fixture.sh extended to eight projects. Usage: capture.sh DIR, from the repository root.
set -eu
d=${1:?usage: capture.sh DIR}
docs/shots/fixture.sh "$d" >/dev/null
d=$(cd "$d" && pwd -P)
for n in ledger search-api mobile-app docs-site data-sync admin-ui; do
  mkdir -p "$d/$n/.taskq"
  sed "s/billing-api/$n/g" "$d/billing-api/.taskq/status.json" > "$d/$n/.taskq/status.json"
  cp "$d/billing-api/.taskq/events" "$d/$n/.taskq/"
  printf '{"board":"github","repo":"acme/%s"}\n' "$n" > "$d/$n/taskq.json"
  touch "$d/$n.log"
  echo "$d/$n" >> "$d/install/projects"
done
TASKQ_INSTALL=$d/install cargo run --locked -- tray --snapshot "$d/shots" --route views
