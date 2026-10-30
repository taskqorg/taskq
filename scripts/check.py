#!/usr/bin/env python3
"""Project-owned selected gate. Unknown impact runs the full Rust suite."""
import argparse
import json
import re
import shutil
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BUDGET = 900


def selection(paths):
    if paths and all(Path(p).suffix in {'.md', '.txt'} and p not in {'taskq.md', 'AGENTS.md', 'CLAUDE.md'} for p in paths):
        # These pages carry published asset hashes, checked by the Node components.
        return 'web' if any(p in {'docs/design/answer-card.md', 'docs/design/shared-resource-controls.md'} for p in paths) else 'docs'
    if paths and all((p.startswith('src/') and Path(p).suffix in {'.js', '.css', '.html'}) or (p.startswith('tests/') and Path(p).suffix == '.cjs') for p in paths):
        return 'web'
    return 'full'


# docs/runtime-qualification.md#runtime-adapters: only these files may name a runtime
ADAPTER_EXCEPTIONS = {'src/runtime.rs', 'src/linux_sandbox.rs', 'src/windows_sandbox.rs'}
# Runtime names, bare or name-prefixed, and the native names only the adapter may know (#1593).
RUNTIME_NAME = re.compile(r'"(claude|codex|codex-app)(:[^"]*)?"|claude\.exe|\.claude\.json|CODEX_HOME|CODEX_THREAD_ID')
TEST_MOD = re.compile(r'#\[cfg\(test\)\]\s*(pub(\([\w:]+\))?\s+)?mod\s+\w+\s*\{')


def runtime_names(path, text):
    """Runtime name literals, bare or name-prefixed, in non-test Rust outside the runtime adapter and the named exceptions."""
    if path in ADAPTER_EXCEPTIONS or path.startswith(('src/runtime/', 'src/windows_sandbox/')) or path.endswith('_tests.rs'):
        return []
    # Blank only `#[cfg(test)] mod x { .. }` bodies, keeping line numbers; other code stays scanned.
    # ponytail: a module ends at its first column-0 `}`; an early end only adds hits, never hides one
    for m in reversed(list(TEST_MOD.finditer(text))):
        close = text.find('\n}', m.end())
        end = len(text) if close < 0 else close + 2
        text = text[:m.start()] + re.sub(r'[^\n]', ' ', text[m.start():end]) + text[end:]
    return [f'{path}:{n}: {line.strip()}' for n, line in enumerate(text.splitlines(), 1) if RUNTIME_NAME.search(line)]


# docs/design/rule-ids.md#contract-detail: rule IDs, the rule-change gate and coverage.
RULE_DOCS = ('taskq.md', 'docs/design/menu.md')
RULE_TESTS = ('src', 'tests', 'scripts')
TEST_PATH = re.compile(r'^(src/.*\.rs|tests/.*|scripts/.*\.test\.py)$')
ASSERTION = re.compile(r'\bassert|\bexpect\(|Chain:')
MEANING_WORDS = {'not', 'no', 'never', 'only', 'now', 'all', 'any', 'none', 'must', 'may'}


def rule_blocks(path, text):
    """Rule ID -> whitespace-normalized text; text without its own ID joins the enclosing ID."""
    blocks, cur, sec, num, sub = {}, None, None, None, None
    menu = path.endswith('menu.md')
    for line in text.splitlines():
        title = re.sub(r'\s*\([^)]*\)\s*$', '', line.lstrip('#')).strip()
        if line.startswith('## '):
            m = re.match(r'(M\d+)\b' if menu else r'(\d+)\.', title)
            num = m and m.group(1)
            sec = cur = (num if menu else f'§{num}') if num else (None if menu else title)
            sub = None
        elif line.startswith('### ') and not menu and sec:
            m = re.match(r'(R\d+)\.', title)
            cur, sub = (m.group(1) if m else f'§{num} {title}' if num else title), None
        elif not line.strip() or line.startswith(('#', '- ')):
            sub = None
        if cur is None or re.match(r'\[[^\]]+\]:\s', line):
            continue
        row = re.match(r'\|.*\|\s*(\d+)\s*\|\s*$', line) if not menu and cur == sec and num else None
        own = re.match(r'(?:- )?(M\d+\.\d+) ', line) if menu else None
        if row:
            key = f'{cur}.{row.group(1)}'
        elif own:
            key = sub = own.group(1)
        else:
            key = sub or cur
        blocks[key] = f'{blocks.get(key, "")} {line}'
    return {k: ' '.join(v.split()) for k, v in blocks.items()}


def rule_order(rule):
    return [int(p) if p.isdigit() else p for p in re.split(r'(\d+)', rule)]


def doc_rules(docs):
    return {k: v for path, text in docs.items() for k, v in rule_blocks(path, text).items()}


def edit_distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        prev, row[0] = row[0], i
        for j, y in enumerate(b, 1):
            prev, row[j] = row[j], min(row[j] + 1, row[j - 1] + 1, prev + (x != y))
    return row[-1]


def typo_only(old, new):
    """At most two words each replaced at edit distance <= 2 and under half its length; no digits, code, links or meaning words."""
    a, b = old.split(), new.split()
    if len(a) != len(b):
        return False
    pairs = [(x, y) for x, y in zip(a, b) if x != y]
    plain = lambda w: not re.search(r'[\d`\[\]()<>|]', w) and w.strip('.,;:!?*').lower() not in MEANING_WORDS
    return len(pairs) <= 2 and all(plain(x) and plain(y) and edit_distance(x, y) <= min(2, (max(len(x), len(y)) - 1) // 2) for x, y in pairs)


def id_pattern(rule):
    body = r'\s+'.join(re.escape(w) for w in rule.split()).replace('§', r'(?:§ ?|[Ss]ection\s+)')
    return re.compile(r'(?<![\w.])' + body + r'(?!\w|\.\d)')


def cited(text, rules):
    """IDs cited in text; a longer ID's match never also cites a shorter one (`§2 Ask card` is not `§2`)."""
    found = set()
    for rule in sorted(rules, key=len, reverse=True):
        pattern = id_pattern(rule)
        if pattern.search(text):
            found.add(rule)
            text = pattern.sub(' ', text)
    return found


def rule_findings(base_docs, head_docs, test_diff):
    """Changed and removed rule IDs, and IDs cited by removed or changed test assertions and Chain: lines."""
    old, new = doc_rules(base_docs), doc_rules(head_docs)
    changed = sorted((k for k in old if k in new and old[k] != new[k] and not typo_only(old[k], new[k])), key=rule_order)
    removed = sorted((k for k in old if k not in new), key=rule_order)
    added = {' '.join(l[1:].split()) for l in test_diff.splitlines() if l.startswith('+') and not l.startswith('+++')}
    lost = [l[1:] for l in test_diff.splitlines() if l.startswith('-') and not l.startswith('---')
            and ASSERTION.search(l) and ' '.join(l[1:].split()) not in added]
    tests = sorted(cited('\n'.join(lost), old.keys() | new.keys()), key=rule_order)
    return {'changed': changed, 'removed': removed, 'tests': tests}


def unnamed(findings, task_text):
    """Findings the task text does not name; a removal also needs `<ID> -> <destination>`."""
    named = cited(task_text, set(sum(findings.values(), [])))
    missing = [f'{k} ({kind})' for kind in ('changed', 'tests') for k in findings[kind] if k not in named]
    for k in findings['removed']:
        if not re.search(id_pattern(k).pattern + r'\s*(?:->|→)\s*\S', task_text):
            missing.append(f'{k} (removed; name `{k} -> <ID, file or none>`)')
    return missing


def uncovered(rules, chain_text):
    return sorted(set(rules) - cited(chain_text, rules), key=rule_order)


def git_docs(rev):
    def show(path):
        done = subprocess.run(['git', 'show', f'{rev}:{path}'], cwd=ROOT, capture_output=True)
        return done.stdout.decode() if done.returncode == 0 else ''
    return {p: show(p) for p in RULE_DOCS}


def test_diff(base, head=None):
    args = ['git', 'diff', '-U0', base, *([head] if head else []), '--', *RULE_TESTS]
    out = subprocess.check_output(args, cwd=ROOT).decode(errors='replace')
    keep, files = [], ''
    for line in out.splitlines():
        if line.startswith('diff --git '):
            files = line.split(' b/', 1)[-1]
        elif TEST_PATH.match(files):
            keep.append(line)
    return '\n'.join(keep)


def approval_text(comments, task):
    """An owner's picked answer approves its ask context and chosen option, never the other options."""
    parts, pending, seen = [], None, set()
    state, attempt_id, resume, last, retries = 'ready', None, None, 0, 0
    for c in sorted(comments, key=lambda c: int(c['id'])):
        if c.get('author_association') not in {'OWNER', 'MEMBER', 'COLLABORATOR'} or not c.get('created_at') or c['created_at'] != c.get('updated_at'):
            continue
        body = c['body'].strip()
        lines = body.splitlines()
        if not lines:
            continue
        revision = None
        if re.fullmatch(r'revision: [0-9]+', lines[-1].strip()) and len(lines) > 1 and lines[-2].strip().startswith('op-id: '):
            revision = int(lines.pop().split(': ', 1)[1])
        if lines[-1].strip().startswith('op-id: '):
            op = lines.pop().strip().split(': ', 1)[1].strip()
            if op in seen:
                continue
            seen.add(op)
        if not lines:
            continue
        head = lines[0].strip()
        if head.startswith('taskq:'):
            head = 'taskq: ' + ' '.join(head[len('taskq:'):].split())
        attempt = re.search(r' attempt ([0-9]+)$', head)
        own = int(attempt[1]) if attempt else None
        kind = re.fullmatch(r'taskq: (\S+)(?: .*?)?(?: attempt [0-9]+)?', head)
        kind = kind[1] if kind else None
        if own is not None and own != attempt_id:
            continue
        if state in {'closed', 'unknown'}:
            continue
        canonical = re.fullmatch(r'taskq: answer(?: ([0-9]+))?(?: attempt [0-9]+)?', head)
        alias = re.fullmatch(r'taskq answer\s+(?:#?([0-9]+)\.)?([0-9]+)', head)
        if alias and alias[1] is not None and int(alias[1]) != int(task):
            continue
        if revision is not None and revision != (pending[0] if pending else attempt_id or last):
            continue
        # Prove only ordinary ask lifecycles. Unmodelled transitions discard context;
        # they never turn an ignored ask or spawn into specification approval.
        spawn = re.fullmatch(r'taskq: spawn (worker|reviewer) \S+ \S+', head)
        if spawn:
            if (spawn[1] == 'worker' and state == 'ready') or (spawn[1] == 'reviewer' and state == 'review' and attempt_id is None):
                state, attempt_id, pending = ('doing' if spawn[1] == 'worker' else 'review'), int(c['id']), None
                last = int(c['id'])
            continue
        if kind == 'ask' and own is not None and state in {'doing', 'review'}:
            opts = {}
            for line in lines[1:]:
                if m := re.fullmatch(r'\+?([0-9]+)\. (.+)', line.strip()):
                    opts.setdefault(int(m[1]), m[2].rstrip('★').strip())
            pending = (int(c['id']), lines[1:], opts)
            resume, state = state, 'ask'
            continue
        if canonical and own is None:
            parts.append('\n'.join(lines))
        if re.fullmatch(r'taskq: result [0-9a-fA-F]{40} attempt [0-9]+', head) and state == 'doing' and own is not None:
            state, attempt_id, pending = 'review', None, None
            last = int(c['id'])
        elif head == 'taskq: hold' and state == 'ready' and last == 0:
            state = 'held'
            last = int(c['id'])
        elif head == 'taskq: start' and state == 'held':
            state = 'ready'
            last = int(c['id'])
        elif kind == 'requeue' and own is None and state in {'ready', 'doing', 'review', 'ask', 'later', 'failed'}:
            pending = None
            state, attempt_id, last, retries = 'ready', None, int(c['id']), 0
        elif kind == 'requeue' and own is not None and state in {'doing', 'review'}:
            # § 2 row 9: an attempt requeue readies the task; its 3rd retry asks with the unmodelled limit card.
            retries += not head.startswith('taskq: requeue main-moved ')
            pending = None
            state, attempt_id, last = ('ready' if retries < 3 else 'unknown'), None, int(c['id'])
        elif kind in {'later', 'failed'} and state in {'ready', 'doing', 'review', 'ask'}:
            pending = None
            state, attempt_id, last = (kind if own is None else 'unknown'), None, int(c['id'])
        elif kind == 'close' and state in {'review', 'later', 'failed'}:
            state, attempt_id, pending = 'closed', None, None
        elif kind in {'gone', 'usage-limit', 'publish', 'publication-blocked', 'publication-failed', 'rebased'} and state in {'doing', 'review'}:
            pending = None
            state, attempt_id = 'unknown', None
        if not pending:
            continue
        payload = '\n'.join(lines[1:] if canonical else lines).strip().rstrip('★').strip()
        opts = pending[2]
        picked = int(canonical[1]) if canonical and canonical[1] else int(alias[2]) if alias else int(payload) if payload.isascii() and payload.isdecimal() else None
        if picked is None:
            matches = [k for k, text in opts.items() if payload.casefold() in {text.casefold(), f'{k}. {text}'.casefold()}]
            if len(matches) == 1:
                picked = matches[0]
            elif canonical:
                picks = set(re.findall(r'(?:^|[.!]\s+)(?:I\s+)?(?:Pick|Choose)\s+([1-9][0-9]*)(?=\s*(?:[.(]|$))', payload, re.I | re.M))
                if len(picks) == 1:
                    picked = int(picks.pop())
        if picked in opts and own is None:
            context = [line for line in pending[1] if not re.match(r'\+?[0-9]+\. |To answer:', line.strip())]
            parts.extend(['\n'.join(context), opts[picked]])
        if kind == 'answer' or alias or picked in opts:
            pending, retries = None, 0
            state = resume
            last = int(c['id'])
    return '\n'.join(parts)


def task_text(base):
    """Issue bodies plus owner decisions with their selected asks; None when the board evidence is unknown."""
    log = subprocess.check_output(['git', 'log', '--format=%B', f'{base}..HEAD'], cwd=ROOT).decode()
    tasks = sorted(set(re.findall(r'^Task:\s*https://github\.com/([\w.-]+/[\w.-]+)/issues/(\d+)\s*$', log, re.M)))
    if not tasks:
        return None, f'no `Task: https://github.com/<repo>/issues/<N>` line in commits {base}..HEAD'
    parts = []
    for repo, n in tasks:
        for args in (['--jq', '.body // ""', f'repos/{repo}/issues/{n}'],
                     ['--paginate', '--slurp', f'repos/{repo}/issues/{n}/comments']):
            done = subprocess.run(['gh', 'api', *args], cwd=ROOT, capture_output=True, timeout=60)
            if done.returncode:
                return None, f'{repo}#{n} unreadable: {done.stderr.decode(errors="replace").strip()[:200]}'
            try:
                out = done.stdout.decode()
                parts.append(approval_text([c for page in json.loads(out) for c in page], n) if '--slurp' in args else out)
            except (ValueError, TypeError, KeyError) as error:
                return None, f'{repo}#{n} approval evidence unreadable: {error}'
    return '\n'.join(parts), None


def rule_gate(base):
    """Print the rule-change gate and the coverage report; return the gate failure, or None."""
    head_docs = {p: (ROOT / p).read_text(encoding='utf-8') if (ROOT / p).exists() else '' for p in RULE_DOCS}
    chains = '\n'.join(l for p in RULE_TESTS for f in sorted((ROOT / p).rglob('*')) if f.is_file()
                       and TEST_PATH.match(f.relative_to(ROOT).as_posix())
                       for l in f.read_text(encoding='utf-8', errors='replace').splitlines() if 'Chain:' in l)
    rules = doc_rules(head_docs)
    missing = uncovered(rules, chains)
    print(f'Rule coverage: {len(missing)} of {len(rules)} rule IDs have no test Chain: citation (report only): {", ".join(missing)}')
    findings = rule_findings(git_docs(base), head_docs, test_diff(base))
    listed = sum(findings.values(), [])
    if not listed:
        print('Rule gate: no rule or rule-citing test changed')
        return None
    print('Rule gate: ' + '; '.join(f'{kind} {", ".join(v)}' for kind, v in findings.items() if v))
    text, unknown = task_text(base)
    if text is None:
        return f'rule gate unknown (R12): {unknown}'
    left = unnamed(findings, text)
    return f'rule gate: the task does not name {", ".join(left)} (docs/design/rule-ids.md#contract-detail)' if left else None


def changes(base):
    args = ['git', 'diff', '--name-only', '-z', base, '--']
    changed = subprocess.check_output(args, cwd=ROOT).decode().split('\0')
    changed += subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '-z'], cwd=ROOT).decode().split('\0')
    return sorted(set(filter(None, changed)))


def check_environment(environment):
    env = dict(environment)
    if os.name == 'nt' and not shutil.which('sh', path=env.get('PATH')):
        git = shutil.which('git', path=env.get('PATH'))
        shell = Path(git).resolve().parent.parent / 'usr' / 'bin' if git else None
        if shell and (shell / 'sh.exe').is_file():
            env['PATH'] = env.get('PATH', '') + os.pathsep + str(shell)
    return env


def test_environment(environment, install):
    """Owner-operation fixtures use their own installation, never the launching worker's identity or credentials."""
    env = dict(environment)
    for key in ('TASKQ_TASK', 'TASKQ_ATTEMPT', 'TASKQ_ROLE', 'TASKQ_ROOT', 'CODEX_THREAD_ID', 'CODEX_SESSION_ID',
                'CLAUDE_CODE_SESSION_ID', 'COREQ_TRACE_CONTEXT', 'GH_TOKEN', 'GITHUB_TOKEN', 'GITLAB_TOKEN', 'TASKQ_INCIDENT_REPORTS'):
        env.pop(key, None)
    return {**env, 'TASKQ_INSTALL': str(install)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', default=os.environ.get('CHECK_BASE', 'HEAD^'))
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--plan', action='store_true', help='print selection without running checks')
    parser.add_argument('--rules', action='store_true', help='run only the rule-change gate and coverage report')
    args = parser.parse_args()
    if args.rules:
        failed = rule_gate(args.base)
        if failed:
            raise SystemExit(failed)
        return
    try:
        paths = changes(args.base)
        scope = 'full' if args.full or os.environ.get('CHECK_FULL') == 'true' else selection(paths)
    except (subprocess.CalledProcessError, UnicodeError):
        paths, scope = [], 'full'
    if args.plan:
        print('scope='+scope)
        return
    print('Selected checks:', scope, '| base:', args.base, flush=True)
    started = time.monotonic()
    env = check_environment(os.environ)

    def run(command, extra_env=None, environment=None):
        left = BUDGET - (time.monotonic() - started)
        if left <= 0:
            raise TimeoutError('selected gate exceeded 900s including setup/build')
        print('+', ' '.join(command), flush=True)
        child_env = {**(env if environment is None else environment), **(extra_env or {})}
        command = [shutil.which(command[0], path=child_env.get('PATH')) or command[0], *command[1:]]
        if os.name == 'nt' and Path(command[0]).stem == 'sh':
            child_env['PATH'] = str(Path(command[0]).parent) + os.pathsep + child_env.get('PATH', '')
        subprocess.run(command, cwd=ROOT, env=child_env, check=True, timeout=left)

    try:
        run(['git', 'diff', '--check', args.base if paths else 'HEAD'])
        run([sys.executable, 'scripts/check.test.py'])
        # CI runs the rule gate once, in the plan job; local runs include it.
        failed = None if os.environ.get('GITHUB_ACTIONS') else rule_gate(args.base)
        if failed:
            raise SystemExit(failed)
        found = [f for p in sorted(ROOT.glob('src/**/*.rs')) for f in runtime_names(p.relative_to(ROOT).as_posix(), p.read_text(encoding='utf-8'))]
        if found:
            raise SystemExit('runtime name outside the runtime adapter (docs/runtime-qualification.md#runtime-adapters):\n' + '\n'.join(found))
        if scope != 'docs':
            run(['npm', 'ci', '--ignore-scripts', '--no-audit', '--no-fund'])
            run(['npm', 'run', 'build'])
            with tempfile.TemporaryDirectory(prefix='taskq-tests-') as install:
                run(['cargo', '+stable', 'test', '--locked'] + ([] if scope == 'full' else ['--bin', 'taskq', 'core::tests']), environment=test_environment(env, install))
            if scope == 'full':
                with tempfile.TemporaryDirectory(prefix='taskq-gate-') as install:
                    run(['cargo', '+stable', 'run', '--locked', '--', 'selftest'], {'TASKQ_SELFTEST_OFFLINE':'1', 'TASKQ_INCIDENT_REPORTS':'off'}, environment=test_environment(env, install))
            if os.environ.get('RUNNER_OS') in (None, 'Linux'):
                for script in sorted((ROOT / 'tests').glob('*.test.cjs')):
                    run(['node', script.relative_to(ROOT).as_posix()])
                run(['sh', 'docs/shots/check.test.sh'])
                run(['sh', '.github/release-tag.test.sh'])
                run([sys.executable, '.github/release-check.test.py'])
        for path in paths:
            if path.startswith('tests/') and path.endswith('.browser.cjs') and os.environ.get('RUNNER_OS') is None:
                run(['node', path])
    finally:
        print(f'Selected gate elapsed: {time.monotonic() - started:.3f}s / {BUDGET}s; no retries', flush=True)


if __name__ == '__main__':
    main()
