#!/bin/sh
# Chain check for the README shots CI verdict (taskq.md R12): only stale UI sources exit 2 (CI warns); a missing or
# malformed manifest, even one with a valid sourceCommit line, and an unrecorded README image exit 1 (CI fails).
set -eu
src=$(cd "$(dirname "$0")" && pwd)/check.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cd "$t"
git init -q . && git config user.email t@t && git config user.name t && git config core.autocrlf false
mkdir -p src scripts docs/design docs/shots
cp "$src" docs/shots/check.sh
for f in src/tray.rs src/tray.html src/tray-app.js src/tray-entry.js src/task-workspace.js src/task-workspace.css src/tray-view-state.js src/tray-web.js src/app-shell.js src/app-shell.css src/task-dashboard.js src/task-dashboard.css src/task-views.js src/anchored-popover.js src/ui-icons.js src/ui-icons.json src/project-blocks.js src/project-blocks.css src/conversations.rs src/conversations.js src/reports.js src/conversations.css src/pm-composer.js src/ui-runtime.js src/workspace.js src/workspace-entry.js src/workspace-answers.js src/answer-card.js scripts/build-web.cjs src/icon.rs; do echo "$f" > "$f"; done
echo '![](docs/shots/a.png)' > README.md
git add -A && git commit -qm ui
manifest() { # $1: blob recorded for src/tray.rs
  printf '{\n  "sourceCommit": "%s",\n  "uiSources": {\n' "$(git rev-parse HEAD)"
  sep=''
  for f in src/tray.rs src/tray.html src/tray-app.js src/tray-entry.js src/task-workspace.js src/task-workspace.css src/tray-view-state.js src/tray-web.js src/app-shell.js src/app-shell.css src/task-dashboard.js src/task-dashboard.css src/task-views.js src/anchored-popover.js src/ui-icons.js src/ui-icons.json src/project-blocks.js src/project-blocks.css src/conversations.rs src/conversations.js src/reports.js src/conversations.css src/pm-composer.js src/ui-runtime.js src/workspace.js src/workspace-entry.js src/workspace-answers.js src/answer-card.js scripts/build-web.cjs src/icon.rs; do
    b=$(git rev-parse "HEAD:$f"); [ "$f" = src/tray.rs ] && b=$1
    printf '%s    "%s": "%s"' "$sep" "$f" "$b"; sep=',
'
  done
  printf '\n  },\n  "images": [\n    {\n      "file": "a.png"\n    }\n  ]\n}\n'
}
fails=0
case_() { # $1: name, $2: expected exit
  rc=0; docs/shots/check.sh 2>/dev/null || rc=$?
  [ "$rc" = "$2" ] && echo "ok   $1 exit $rc" || { echo "FAIL $1 exit $rc, want $2"; fails=1; }
}
manifest "$(git rev-parse HEAD:src/tray.rs)" > docs/shots/manifest.json
case_ fresh 0
echo 'changed shared shell' > src/app-shell.css
git add src/app-shell.css && git commit -qm shell
case_ 'changed shared shell' 2
git checkout HEAD^ -- src/app-shell.css
git commit -qm restore
manifest 0000000000000000000000000000000000000000 > docs/shots/manifest.json
case_ stale 2
echo '{"garbage' >> docs/shots/manifest.json
case_ 'stale + malformed JSON after a valid sourceCommit' 1
manifest "$(git rev-parse HEAD:src/tray.rs)" | sed 's/"a.png"/"b.png"/' > docs/shots/manifest.json
case_ 'unrecorded README image' 1
rm docs/shots/manifest.json
case_ 'missing manifest' 1
exit $fails
