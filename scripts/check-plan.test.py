"""CI plan stdout is a GitHub-output protocol; human selection diagnostics stay on stderr."""
from pathlib import Path
import os
import re
import subprocess
import sys
import tempfile

runner = Path(__file__).with_name('check.py').read_text(encoding='utf-8')
with tempfile.TemporaryDirectory(prefix='taskq-plan-output-') as directory:
    root = Path(directory)
    for name, body in {'scripts/check.py': runner, 'src/answer-card.js': '// invented component\n',
                       'tests/answer-card.test.cjs': '// invented model\n'}.items():
        file = root / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(body, encoding='utf-8')
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_') and k not in {'CHECK_FULL', 'CHECK_BASE'}}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT='0')
    def git(*args):
        subprocess.run(['git', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
                        '-c', 'user.name=Invented fixture', '-c', 'user.email=fixture@example.invalid', *args],
                       cwd=root, env=env, capture_output=True, text=True, check=True, timeout=10)
    git('init', '-q')
    git('add', '.')
    git('commit', '-qm', 'invented plan base')
    (root / 'src/answer-card.js').write_text('// changed invented component\n', encoding='utf-8')
    # Exact canonical --plan path: changed files come from Git, not an injected selection helper.
    done = subprocess.run([sys.executable, 'scripts/check.py', '--plan', '--base', 'HEAD'],
                          cwd=root, env=env, capture_output=True, text=True, timeout=15)
    assert done.returncode == 0, done.stderr
    records = done.stdout.splitlines()
    assert records == ['scope=web'], (done.stdout, done.stderr)
    assert all(re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*=[^\r\n]*', record) for record in records)
    assert 'web checks: tests/design-sources.test.cjs, tests/answer-card.test.cjs' in done.stderr, done.stderr
    assert 'Rust selection:' in done.stderr, done.stderr
print('PASS actual known-web plan: exact scope output record, selection diagnostics only on stderr')
