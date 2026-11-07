"""Exercise real publisher subprocesses against a shared local gh substitute."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

src = Path(os.environ.get('PUBLISH_SCRIPT', Path(__file__).resolve().with_name('release-publish.sh')))
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    (root / 'bin').mkdir()
    (root / 'store').mkdir()
    stub = root / 'bin/gh'
    stub.write_text('''#!/usr/bin/env python3
import os, sys, shutil, time
from pathlib import Path
r = Path(os.environ['STORE'])
a = sys.argv[1:]
if a[0] == 'api':
    if 'POST' in a:
        with (r/'lock-attempts').open('a') as attempts: attempts.write('attempt\\n')
        try: (r/'lock').mkdir()
        except FileExistsError: sys.exit(1)
    else: (r/'lock').rmdir()
    sys.exit(0)
op = a[1]
if op == 'view':
    if os.environ.get('READ_FAIL'): sys.exit(1)
    print('\\n'.join(p.name for p in (r/'store').iterdir()))
elif op == 'download':
    d = Path(a[a.index('--dir')+1])
    for n in ['SHA256SUMS', 'latest.json']:
        if (r/'store'/n).exists(): shutil.copy(r/'store'/n, d/n)
elif op == 'upload':
    if os.environ.get('FLAKY') and not (r/'flaked').exists():
        (r/'flaked').touch()
        sys.exit(1)
    for f in a[3:]:
        if f.startswith('--') or f == 'o/r': continue
        p = Path(f)
        if p.name == 'latest.json' and os.environ.get('DELAY'):
            (r/'paused').touch()
            while not (r/'resume').exists(): time.sleep(.01)
        shutil.copy(p, r/'store'/p.name)
elif op == 'edit': (r/'edited').touch()
else: sys.exit(2)
''')
    stub.chmod(0o755)
    (root/'bin/shasum').write_text('#!/usr/bin/env python3\nimport hashlib,sys\nfor n in sys.argv[3:]: print(hashlib.sha256(open(n,"rb").read()).hexdigest()+"  "+n)\n')
    (root/'bin/shasum').chmod(0o755)
    env = dict(os.environ, SETTLE='0', RETRY_WAIT='0', STORE=str(root), GITHUB_REPOSITORY='o/r', GITHUB_SHA='a'*40,
               LOCK_WAIT='0.05', PATH=str(root/'bin')+os.pathsep+os.environ['PATH'])
    def start(name, **extra):
        d = root/name
        d.mkdir(exist_ok=True)
        (d/f'taskq-4.261107.10-{name}').write_text(name)
        return subprocess.Popen(['sh', str(src), 'v4.261107.10', str(d)], env=dict(env, **extra))
    def waitfile(name):
        deadline = time.monotonic()+10
        while not (root/name).exists():
            assert time.monotonic()<deadline, name
            time.sleep(.01)
    # Windows pauses after reading empty index; Linux must not finish behind it.
    win = start('windows-x86_64', DELAY='1')
    waitfile('paused')
    linux = start('linux-x86_64')
    try:
        time.sleep(1)
        assert linux.poll() is None, 'Linux bypassed delayed Windows index transaction'
    finally:
        (root/'resume').touch()
    assert win.wait(timeout=10) == linux.wait(timeout=10) == 0
    index = json.loads((root/'store/latest.json').read_text())
    assert index['version'] == '4.261107.10'
    files = index['files']
    assert set(files) == {'windows-x86_64', 'linux-x86_64'}, files
    assert len((root/'store/SHA256SUMS').read_text().splitlines()) == 2
    print('PASS delayed writer preserves both OS indexes')
    before = (root/'store/latest.json').read_bytes()
    assert start('macos-aarch64', READ_FAIL='1').wait(timeout=10) != 0
    assert (root/'store/latest.json').read_bytes() == before
    assert not (root/'lock').exists()
    print('PASS read failure preserves index and releases owned lock')
    (root/'lock').mkdir()
    # The unavailable lock makes 60 separate Python-stub launches; Windows process startup
    # exceeded the old 10s wrapper. Keep the retry count and every no-write/no-steal assertion.
    (root/'lock-attempts').write_text('')
    started = time.monotonic()
    assert start('macos-aarch64', LOCK_WAIT='0').wait(timeout=60 if os.name == 'nt' else 10) != 0
    assert len((root/'lock-attempts').read_text().splitlines()) == 60
    print(f'Unavailable lock: 60 attempts in {time.monotonic()-started:.2f}s')
    assert (root/'lock').exists()
    assert (root/'store/latest.json').read_bytes() == before
    print('PASS unavailable lock refuses writes without stealing')
    (root/'lock').rmdir()
    assert start('macos-aarch64', FLAKY='1').wait(timeout=10) == 0
    assert (root/'flaked').exists()
    assert (root/'store/taskq-4.261107.10-macos-aarch64').exists()
    assert 'macos-aarch64' in json.loads((root/'store/latest.json').read_text())['files']
    assert not (root/'lock').exists()
    print('PASS transient upload failure retries and publishes')
