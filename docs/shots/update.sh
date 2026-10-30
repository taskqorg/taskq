#!/bin/sh
# Refresh the README screenshots: build the current UI and render its isolated showcase fixtures,
# copy the dark images here and write manifest.json (shape of docs/design/evidence/4.1.9/manifest.json).
set -eu
cd "$(dirname "$0")/../.."
ui='src/tray.rs src/tray.html src/tray-app.js src/tray-entry.js src/task-workspace.js src/task-workspace.css src/tray-view-state.js src/tray-web.js src/app-shell.js src/app-shell.css src/task-dashboard.js src/task-dashboard.css src/task-views.js src/anchored-popover.js src/ui-icons.js src/ui-icons.json src/project-blocks.js src/project-blocks.css src/conversations.rs src/conversations.js src/reports.js src/conversations.css src/pm-composer.js src/ui-runtime.js src/workspace.js src/workspace-entry.js src/workspace-answers.js src/answer-card.js scripts/build-web.cjs src/icon.rs'
# the images must come from sourceCommit, so UI sources must be committed
git diff --quiet HEAD -- $ui || { echo "update.sh: commit UI source changes first: $ui" >&2; exit 1; }
cargo build --release --locked
bin=${CARGO_TARGET_DIR:-target}/release/taskq
case $(uname -s) in
  MINGW*|MSYS*) bin=$bin.exe; os=Windows; renderer='WebView2 snapshot'; capture() { "$bin" "$@"; } ;;
  *) os="macOS $(sw_vers -productVersion)"; renderer='WKWebView snapshot'; capture() { scripts/macos-runner.sh "$bin" "$@"; } ;;
esac
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
tmp=$(mktemp -d)
tmp=$(cd "$tmp" && (pwd -W 2>/dev/null || pwd -P))
trap 'rm -rf "$tmp"' EXIT
TASKQ_VERSION=$version docs/shots/fixture.sh "$tmp"
# Keep the storefront scenario consistent across all three examples.
printf '%s\n' "$tmp/storefront" "$tmp/billing-api" > "$tmp/install/projects"
# only the routes the README shows (taskq.md R12)
TASKQ_INSTALL=$tmp/install capture tray --snapshot "$tmp/shots" --route project,icon
TASKQ_INSTALL=$tmp/install capture tray --snapshot "$tmp/conversations" --route conversations
rm -f docs/shots/*.png
cp "$tmp/shots/popover-dark-project-16.png" docs/shots/
cp "$tmp/conversations/popover-dark-conversations-owner-18.png" docs/shots/
cp "$tmp/conversations/popover-dark-conversations-question-18.png" docs/shots/
# Menu-bar size and scale differ between the owner's Mac and CI; keep the README link stable.
cp "$tmp"/shots/icon-summary-run-dark-*.png docs/shots/icon-summary-run-dark.png
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
{
  printf '{\n  "version": "%s",\n  "sourceCommit": "%s",\n  "binarySha256": "%s",\n' "$version" "$(git rev-parse HEAD)" "$(sha "$bin")"
  printf '  "environment": {\n    "os": "%s",\n    "arch": "%s",\n    "renderer": "%s"\n  },\n' "$os" "$(uname -m)" "$renderer"
  printf '  "uiSources": {\n'
  sep=''
  for f in $ui; do printf '%s    "%s": "%s"' "$sep" "$f" "$(git rev-parse "HEAD:$f")"; sep=',
'; done
  printf '\n  },\n  "captureInputs": {\n'
  printf '    "docs/shots/fixture.sh": "%s",\n' "$(sha docs/shots/fixture.sh)"
  printf '    "tests/fixtures/conversation-shots.js": "%s",\n' "$(sha tests/fixtures/conversation-shots.js)"
  printf '    "docs/shots/update.sh": "%s"\n  },\n' "$(sha docs/shots/update.sh)"
  printf '  "capture": "Isolated TASKQ_INSTALL; queue and Conversations showcase fixtures; dark images only.",\n  "images": [\n'
  printf '    {\n      "file": "../header.webp",\n      "sha256": "%s"\n    }' "$(sha docs/header.webp)"
  sep=',
'
  for f in docs/shots/*.png; do
    printf '%s    {\n      "file": "%s",\n      "sha256": "%s"\n    }' "$sep" "${f##*/}" "$(sha "$f")"
    sep=',
'
  done
  printf '\n  ]\n}\n'
} > docs/shots/manifest.json
echo "docs/shots updated from $(git rev-parse --short HEAD)"
