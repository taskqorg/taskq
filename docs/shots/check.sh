#!/bin/sh
# Check of the README screenshots (docs/design/readme-plan.md, Images and acceptance; taskq.md R12): `check.sh [DIR]`,
# DIR defaults to docs/shots. Exit 1, broken (CI fails): the manifest is missing or malformed JSON, a UI source is not in
# HEAD, an image it lists is missing, is not a PNG or does not match its sha256, or README.md references an image it
# lacks. Exit 2, stale (CI warns; the README shots workflow refreshes): a UI source blob, a capture input or the
# Cargo.toml version differs from the manifest. Sources, inputs and version are read from HEAD, not the working tree,
# so a squash merge dropping sourceCommit and line-ending conversion change nothing. docs/shots/update.sh runs it on
# its staged set and requires exit 0.
set -eu
cd "$(dirname "$0")/../.."
exec python3 - "${1:-docs/shots}" <<'PY'
import hashlib, json, re, subprocess, sys
from pathlib import Path

UI = '''src/tray.rs src/tray.html src/tray-app.js src/tray-entry.js src/task-workspace.js src/task-workspace.css
src/tray-view-state.js src/tray-web.js src/app-shell.js src/app-shell.css src/task-dashboard.js src/task-dashboard.css
src/task-views.js src/anchored-popover.js src/ui-icons.js src/ui-icons.json src/project-blocks.js src/project-blocks.css
src/conversations.rs src/conversations.js src/reports.js src/conversations.css src/pm-composer.js src/ui-runtime.js
src/workspace.js src/workspace-entry.js src/workspace-answers.js src/answer-card.js scripts/build-web.cjs src/icon.rs'''.split()
INPUTS = ['docs/shots/fixture.sh', 'tests/fixtures/conversation-shots.js', 'docs/shots/update.sh', 'Cargo.lock']
d = Path(sys.argv[1])
m = d / 'manifest.json'


def git(*args):
    r = subprocess.run(['git', *args], capture_output=True)
    return r.stdout if r.returncode == 0 else None


try:
    man = json.loads(m.read_text(encoding='utf-8'))
    assert re.fullmatch('[0-9a-f]{40}', man['sourceCommit'])
except Exception:
    sys.exit(f'check.sh: {m} is missing, malformed or has no sourceCommit')
broken, stale = [], []
for f in UI:
    blob = git('rev-parse', '--verify', '-q', f'HEAD:{f}')
    if blob is None:
        broken.append(f'{f} is not in HEAD')
    elif man.get('uiSources', {}).get(f) != blob.decode().strip():
        stale.append(f'{f} changed since manifest sourceCommit')
for f in INPUTS:
    data = git('cat-file', 'blob', f'HEAD:{f}')
    if data is None or man.get('captureInputs', {}).get(f) != hashlib.sha256(data).hexdigest():
        stale.append(f'capture input {f} differs from the manifest')
version = re.search(r'^version = "(.*)"', (git('show', 'HEAD:Cargo.toml') or b'').decode(), re.M)
version = version and version.group(1)
if man.get('version') != version:
    stale.append(f'manifest version {man.get("version")} is not the Cargo.toml version {version}')
images = {i.get('file'): i.get('sha256') for i in man.get('images', [])}
for name, want in images.items():
    p = d / str(name)
    try:
        data = p.read_bytes()
    except OSError:
        broken.append(f'image {p} is missing')
        continue
    if hashlib.sha256(data).hexdigest() != want:
        broken.append(f'image {p} does not match its manifest sha256')
    elif p.suffix == '.png' and not data.startswith(b'\x89PNG\r\n\x1a\n'):
        broken.append(f'image {p} is not a PNG')
readme = Path('README.md').read_text(encoding='utf-8') if Path('README.md').exists() else ''
for f in sorted(set(re.findall(r'docs/shots/[A-Za-z0-9@._-]*\.png', readme))):
    if f.rsplit('/', 1)[1] not in images:
        broken.append(f'{f} is in README.md but not in {m}')
for line in broken:
    print(f'check.sh: {line}', file=sys.stderr)
for line in stale:
    print(f'check.sh: stale: {line}; run the README shots workflow', file=sys.stderr)
sys.exit(1 if broken else 2 if stale else 0)
PY
