#!/bin/sh
# Chain check for the README shots CI verdict (taskq.md R12; #2024 A4): a changed UI source, capture input or Cargo.toml
# version exits 2 (stale, CI warns); an image hash mismatch, a missing or non-PNG image, an unrecorded README image and
# a missing or malformed manifest exit 1 (broken, CI fails). A staged DIR is checked like docs/shots.
set -eu
src=$(cd "$(dirname "$0")" && pwd)/check.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cd "$t"
git init -q . && git config user.email t@t && git config user.name t && git config core.autocrlf false
mkdir -p src scripts docs/shots tests/fixtures stage
cp "$src" docs/shots/check.sh
ui=$(python3 -c "import re,sys; print(*re.search(r\"UI = '''(.*?)'''\", open(sys.argv[1]).read(), re.S).group(1).split())" "$src" | tr -d '\r')
for f in $ui docs/shots/fixture.sh docs/shots/update.sh tests/fixtures/conversation-shots.js Cargo.lock; do echo "$f" > "$f"; done
printf '[package]\nname = "taskq"\nversion = "1.2.3"\n' > Cargo.toml
png() { printf '\211PNG\r\n\032\n%s' "$1"; }
png a > docs/shots/a.png
echo '![](docs/shots/a.png)' > README.md
git add -A && git commit -qm ui
manifest() { # $1: shots directory, $2: Python statement that edits the fresh manifest m
  python3 - "$1" "$2" "$ui" <<'PY'
import hashlib, json, pathlib, subprocess, sys
d, edit, ui = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3].split()
git = lambda *a: subprocess.run(['git', *a], capture_output=True, check=True).stdout
sha = lambda b: hashlib.sha256(b).hexdigest()
m = {'version': '1.2.3', 'sourceCommit': git('rev-parse', 'HEAD').decode().strip(),
     'uiSources': {f: git('rev-parse', f'HEAD:{f}').decode().strip() for f in ui},
     'captureInputs': {f: sha(git('cat-file', 'blob', f'HEAD:{f}')) for f in
                       ['docs/shots/fixture.sh', 'tests/fixtures/conversation-shots.js', 'docs/shots/update.sh', 'Cargo.lock']},
     'images': [{'file': 'a.png', 'sha256': sha((d / 'a.png').read_bytes())}]}
exec(edit)
(d / 'manifest.json').write_text(json.dumps(m, indent=2))
PY
}
fails=0
case_() { # $1: name, $2: expected exit, $3: shots directory (default docs/shots)
  rc=0; docs/shots/check.sh ${3:-} 2>/dev/null || rc=$?
  [ "$rc" = "$2" ] && echo "ok   $1 exit $rc" || { echo "FAIL $1 exit $rc, want $2"; fails=1; }
}
manifest docs/shots pass
case_ fresh 0
echo 'changed shared shell' > src/app-shell.css
git add src/app-shell.css && git commit -qm shell
case_ 'changed UI source' 2
git checkout HEAD^ -- src/app-shell.css && git commit -qm restore
case_ 'restored UI source' 0
manifest docs/shots "m['captureInputs']['tests/fixtures/conversation-shots.js'] = '0' * 64"
case_ 'capture input mismatch' 2
manifest docs/shots "del m['captureInputs']['Cargo.lock']"
case_ 'unrecorded capture input' 2
manifest docs/shots "m['version'] = '1.2.2'"
case_ 'manifest version mismatch' 2
manifest docs/shots "m['images'][0]['sha256'] = '0' * 64"
case_ 'image hash mismatch' 1
manifest docs/shots "m['images'][0]['sha256'] = '0' * 64; m['version'] = '1.2.2'"
case_ 'image hash mismatch beats stale' 1
manifest docs/shots "m['images'].append({'file': 'b.png', 'sha256': '0' * 64})"
case_ 'missing image' 1
printf 'not a png' > docs/shots/a.png
manifest docs/shots pass
case_ 'non-PNG image with matching hash' 1
png a > docs/shots/a.png
manifest docs/shots "m['images'][0]['file'] = 'c.png'; (d / 'c.png').write_bytes((d / 'a.png').read_bytes())"
case_ 'unrecorded README image' 1
rm docs/shots/c.png
manifest docs/shots pass
echo '{"garbage' >> docs/shots/manifest.json
case_ 'malformed JSON after a valid manifest' 1
manifest docs/shots "del m['sourceCommit']"
case_ 'manifest without sourceCommit' 1
rm docs/shots/manifest.json
case_ 'missing manifest' 1
png staged > stage/a.png
manifest stage pass
case_ 'fresh staged directory' 0 stage
manifest stage "m['version'] = '1.2.2'"
case_ 'stale staged directory' 2 stage
exit $fails
