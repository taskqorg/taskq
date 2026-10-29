#!/usr/bin/env bash
# #1841 A2: two concurrent UI tasks, each following the task steps, rebase onto each other.
# Usage: concurrent-rebase.sh <repo with node_modules> <empty temp dir> <old base> <new base>
set -euo pipefail
W=$1 T=$2
run() {
  local base=$1 label=$2; local d=$T/$label
  rm -rf "$d"; git clone -q "$W" "$d"; cd "$d"; git checkout -q -B base "$base"
  if command -v cygpath >/dev/null; then cmd //c "mklink /J $(cygpath -w "$d/node_modules") $(cygpath -w "$W/node_modules")" >/dev/null; else ln -s "$W/node_modules" "$d/node_modules"; fi
  for br in task-a task-b; do
    git checkout -q -b $br base
    if [ $br = task-a ]; then sed -i '1i // task-a change' src/task-workspace.js; else echo '// task-b change' >> src/task-workspace.js; fi
    npm run build >/dev/null
    # The old base requires refreshing the pinned hashes; the new one derives them in the build.
    if [ $label = old ]; then node tests/task-workspace.test.cjs --write; fi
    node tests/task-workspace.test.cjs >/dev/null
    git add -A; git commit -qm $br
    echo "$label $br commits: $(git show --name-only --format= HEAD | tr '\n' ' ')"
  done
  git checkout -q task-b
  if git rebase -q task-a >/dev/null 2>&1; then echo "$label rebase task-b onto task-a: clean, $(git log --oneline base..HEAD | wc -l) commits"
  else echo "$label rebase task-b onto task-a: CONFLICT in $(git diff --name-only --diff-filter=U | tr '\n' ' ')"; git rebase --abort; fi
}
run "$3" old
run "$4" new
