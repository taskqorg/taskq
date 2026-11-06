#!/usr/bin/env python3
"""Project-owned selected gate. Unknown impact runs the full Rust suite."""
import argparse
import hashlib
import http.client
import json
import re
import shutil
import ssl
import os
from pathlib import Path
import subprocess
import sys
import time
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BUDGET = 900
# taskq.md Product: this page is contract text under that ID, so it keeps the root contract's gate and lane.
PRODUCT_DOC = 'docs/design/product-contract.md'


def selection(paths):
    if paths and all(Path(p).suffix in {'.md', '.txt'} and p not in {'taskq.md', 'AGENTS.md', 'CLAUDE.md', PRODUCT_DOC} for p in paths):
        # These pages carry published asset hashes, checked by the Node components.
        return 'web' if any(p in {'docs/design/answer-card.md', 'docs/design/shared-resource-controls.md'} for p in paths) else 'docs'
    if paths and all((p.startswith('src/') and Path(p).suffix in {'.js', '.css', '.html'}) or (p.startswith('tests/') and Path(p).suffix == '.cjs') for p in paths):
        return 'web'
    return 'full'


# docs/runtime-qualification.md#runtime-adapters: the one data table that holds runtime names as values.
RUNTIME_TABLE = 'src/runtime/table.rs'
# Runtime names, bare or name-prefixed, and the native names only the table may know (#1593).
RUNTIME_NAME = re.compile(r'"(claude|codex|codex-app|antigravity)(:[^"]*)?"|claude\.exe|\.claude\.json|CODEX_HOME|CODEX_THREAD_ID')
TEST_MOD = re.compile(r'#\[cfg\(test\)\]\s*(pub(\([\w:]+\))?\s+)?mod\s+\w+\s*\{')
# #2271: a runtime name is table data, never a branch, in the table too: code asks the
# adapter's capabilities (`of(rt).has(..)`) instead of comparing or matching a name.
_NAMES = r'"(claude|codex|codex-app|antigravity|agy)"'
RUNTIME_BRANCH = re.compile(rf'(==|!=)\s*{_NAMES}|{_NAMES}\s*(==|!=|=>|\|)|matches!\([^)]*{_NAMES}')


def runtime_names(path, text):
    """Runtime name literals, bare or name-prefixed, in non-test Rust outside the data table;
    runtime-name branches in all non-test Rust."""
    if path.endswith('_tests.rs'):
        return []
    table = path == RUNTIME_TABLE
    patterns = [RUNTIME_BRANCH] if table else [RUNTIME_NAME, RUNTIME_BRANCH]
    # Blank only `#[cfg(test)] mod x { .. }` bodies, keeping line numbers; other code stays scanned.
    # ponytail: a module ends at its first column-0 `}`; an early end only adds hits, never hides one
    for m in reversed(list(TEST_MOD.finditer(text))):
        close = text.find('\n}', m.end())
        end = len(text) if close < 0 else close + 2
        text = text[:m.start()] + re.sub(r'[^\n]', ' ', text[m.start():end]) + text[end:]
    return [f'{path}:{n}: {line.strip()}' for n, line in enumerate(text.splitlines(), 1) if any(p.search(line) for p in patterns)]


# docs/design/rule-ids.md#contract-detail: rule IDs, the rule-change gate and coverage.
OUTSIDE_TREE = re.compile(r'^docs/(design/(evidence|review-evidence|exploration)/|evidence/|shots/evidence/|reviews/|qualification-|release-)')
RULE_DOCS = ('taskq.md', 'docs/design/menu.md', PRODUCT_DOC)
RULE_TESTS = ('src', 'tests', 'scripts')
TEST_PATH = re.compile(r'^(src/.*\.rs|tests/.*|scripts/.*\.test\.py)$')
ASSERTION = re.compile(r'\bassert|\bexpect\(|Chain:')
MEANING_WORDS = {'not', 'no', 'never', 'only', 'now', 'all', 'any', 'none', 'must', 'may'}


def rule_blocks(path, text):
    """Rule ID -> whitespace-normalized text; text without its own ID joins the enclosing ID."""
    if path == PRODUCT_DOC:
        return {'Product': ' '.join(text.split())} if text.strip() else {}
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
    rules = {}
    for path, text in docs.items():
        for k, v in rule_blocks(path, text).items():
            rules[k] = f'{rules[k]} {v}' if k in rules else v
    return rules


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
    body = r'\s+'.join(re.escape(w) for w in rule.split()).replace('§', r'(?:§ ?|[Ss]ection\s*)')
    return re.compile(r'(?<![\w.])' + body + r'(?!\w|\.\d)')


def cited(text, rules):
    """IDs cited in text; a longer ID's match never also cites a shorter one (`§2 Ask card` is not `§2`)."""
    # Owners also cite paired sections, e.g. "sections 4 Filter and 9"; keep any subsection title for longest-match fencing.
    text = re.sub(r'\bsections\s*(\d+(?:\s+[A-Za-z]+)?)(?:\s+and\s+|\s*/\s*)(\d+)\b', r'section \1 and section \2', text, flags=re.I)
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


def in_scope(rule, bodies):
    """§1 Task text: an issue body's `Rules: <IDs|all>` line (`add --rules`) declares the changed rule IDs; an ID covers
    its rows and subsections. Only issue bodies count: ask context and answers can quote a `Rules:` line."""
    for line in re.findall(r'^Rules: (.+)$', bodies, re.M):
        if line.strip() == 'all':
            return True
        for scope in re.split(r'\s*,\s*', line.strip()):
            scope = re.sub(r'^(?:§\s*|[Ss]ection\s*)(?=\d)', '§', scope)
            if scope and (rule == scope or rule.startswith((scope + '.', scope + ' '))):
                return True
    return False


def unnamed(findings, task_text, bodies=''):
    """Findings the task text neither names nor `bodies` declare in scope; a removal also needs `<ID> -> <destination>`."""
    named = cited(task_text, set(sum(findings.values(), [])))
    missing = [f'{k} ({kind})' for kind in ('changed', 'tests') for k in findings[kind] if k not in named and not in_scope(k, bodies)]
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
    """Accepted owner steering and picked answers bind the task; unchosen options never do."""
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
        if head == 'taskq: steer' and own is None and '\n'.join(lines[1:]).strip():
            # Held intake is an accepted note too: it advances the revision, but grants no rule scope.
            last = int(c['id'])
            if state == 'doing':
                parts.append('\n'.join(lines))
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
            # §2 row 24: plain Later ends the attempt; row 23: a worker `later <ref> attempt` park readies it.
            # ponytail: `waiting` folds as `ready`; dependency state is board data this gate never reads.
            park = kind == 'later' and own is not None and state in {'doing', 'review'} and head != f'taskq: later attempt {attempt_id}'
            terminal = own is None or head == f'taskq: later attempt {attempt_id}'
            state, attempt_id, last = ('ready' if park else kind if terminal else 'unknown'), None, int(c['id'])
        elif kind == 'close' and state in {'review', 'later', 'failed'}:
            state, attempt_id, pending = 'closed', None, None
        elif kind == 'gone' and own is not None and state == 'doing':
            # §2 row 10: a dead worker readies the task; its 2nd gone asks with the unmodelled limit card.
            retries += 1
            pending = None
            state, attempt_id, last = ('ready' if retries < 2 else 'unknown'), None, int(c['id'])
        elif kind in {'gone', 'usage-limit', 'publish', 'publication-blocked', 'publication-failed', 'rebased'} and state in {'doing', 'review'}:
            pending = None
            state, attempt_id = 'unknown', None
        if not pending:
            continue
        payload = '\n'.join(lines[1:] if canonical else lines).strip().rstrip('★').strip()
        opts = pending[2]
        picked = int(canonical[1]) if canonical and canonical[1] else int(alias[2]) if alias else int(payload) if payload.isascii() and payload.isdecimal() else None
        # `taskq answer N --text "N.K note"` (#2202): a leading N.K pick of this task, parsed as `answer N.K` parses it.
        lead = re.match(r'#?([0-9]+)\.([0-9]+)(?:\s|$)', payload) if canonical and not canonical[1] else None
        if picked is None and lead and int(lead[1]) == int(task):
            picked = int(lead[2])
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
    """Issue bodies plus owner decisions with their selected asks, and the bodies alone; None when the board evidence is unknown."""
    log = subprocess.check_output(['git', 'log', '--format=%B', f'{base}..HEAD'], cwd=ROOT).decode()
    tasks = sorted(set(re.findall(r'^Task:\s*https://github\.com/([\w.-]+/[\w.-]+)/issues/(\d+)\s*$', log, re.M)))
    if not tasks:
        return None, '', f'no `Task: https://github.com/<repo>/issues/<N>` line in commits {base}..HEAD'
    parts, bodies = [], []
    for repo, n in tasks:
        for args in (['--jq', '.body // ""', f'repos/{repo}/issues/{n}'],
                     ['--paginate', '--slurp', f'repos/{repo}/issues/{n}/comments']):
            done = subprocess.run(['gh', 'api', *args], cwd=ROOT, capture_output=True, timeout=60)
            if done.returncode:
                return None, '', f'{repo}#{n} unreadable: {done.stderr.decode(errors="replace").strip()[:200]}'
            try:
                out = done.stdout.decode()
                parts.append(approval_text([c for page in json.loads(out) for c in page], n) if '--slurp' in args else out)
                if '--slurp' not in args:
                    bodies.append(out)
            except (ValueError, TypeError, KeyError) as error:
                return None, '', f'{repo}#{n} approval evidence unreadable: {error}'
    return '\n'.join(parts), '\n'.join(bodies), None


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
    text, bodies, unknown = task_text(base)
    if text is None:
        return f'rule gate unknown (R12): {unknown}'
    left = unnamed(findings, text, bodies)
    return f'rule gate: the task does not name {", ".join(left)} (docs/design/rule-ids.md#contract-detail)' if left else None


# taskq.md R8: the shared contract lint (specq engineering/contracts.md#lint) at the pinned specq commit.
SPECQ = '269a999c4e7c54781260a70305cff5f7cecdf572'
CONTRACT_LINT = Path('engineering/scripts/contract_lint.py')
CONTRACT_LINT_SHA256 = 'f88032eb1210b9fadd98ee250c7371a732a6c7837c3917c026b974909f830835'
LINT_LIMIT = 128 * 1024


def tree_evidence(paths):
    """Tracked paths that belong outside the tree: evidence, finished reviews and release records (docs/testing.md)."""
    return [p for p in paths if OUTSIDE_TREE.match(p)]


def main_checkout(root):
    """The main checkout of a linked worktree, read from its `.git` file; None for anything else."""
    try:
        gitdir = root / (root / '.git').read_text(encoding='utf-8').removeprefix('gitdir:').strip()
        return (gitdir / (gitdir / 'commondir').read_text(encoding='utf-8').strip()).resolve().parent
    except OSError:
        return None


def download_lint(url):
    """Python may lack a CA bundle (python.org macOS builds); curl then validates against the OS trust store."""
    try:
        with urllib.request.urlopen(url, timeout=10) as response:
            return response.read(LINT_LIMIT + 1)
    except urllib.error.URLError as error:
        if not isinstance(error.reason, ssl.SSLCertVerificationError):
            raise
    # -q (first) skips any .curlrc, which could weaken validation. A redirect yields no body and fails the hash.
    done = subprocess.run(['curl', '-q', '--fail', '--silent', '--show-error', '--max-time', '10',
                           '--max-filesize', str(LINT_LIMIT), url], capture_output=True, timeout=15)
    if done.returncode:
        raise OSError(f'curl exit {done.returncode}: {done.stderr.decode(errors="replace").strip()}')
    return done.stdout[:LINT_LIMIT + 1]


def contract_lint(root=ROOT):
    """Resolve the verified pinned linter, preparing the local cache once when absent."""
    cached = root / '.taskq' / 'specq' / SPECQ / CONTRACT_LINT
    try:
        main = main_checkout(root)
        # The first copy found decides: an invalid one refuses, never falls through. The main checkout is read only.
        copies = [root / '.specq' / CONTRACT_LINT, cached]
        if main:
            copies.append(main / '.taskq' / 'specq' / SPECQ / CONTRACT_LINT)
        source = next((path for path in copies if path.is_file()), None)
        path = source or cached
        if source:
            body = path.read_bytes()
        else:
            body = download_lint(f'https://raw.githubusercontent.com/taskqorg/specq/{SPECQ}/{CONTRACT_LINT.as_posix()}')
        if hashlib.sha256(body).hexdigest() != CONTRACT_LINT_SHA256:
            raise ValueError('pinned linter SHA-256 mismatch')
        if source is None:
            path.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.TemporaryDirectory(prefix='.lint-', dir=path.parent) as stage:
                prepared = Path(stage) / 'contract_lint.py'
                prepared.write_bytes(body)
                os.replace(prepared, path)
        return [sys.executable, str(path), 'taskq.md']
    except (OSError, ValueError, subprocess.SubprocessError, http.client.HTTPException) as error:
        raise SystemExit(f'contract lint unknown (R12): cannot prepare specq {SPECQ}: {error}') from error


def compact_warnings(output, ci=False):
    """taskq.md R8: each specq `COMPACT:` marker (above the 70% target of the declared budget) becomes a visible warning."""
    prefix = '::warning::' if ci else 'Warning: '
    return [f'{prefix}{line.strip()}; file one compaction task (specq engineering/contracts.md#size-budgets)'
            for line in output.splitlines() if line.startswith('COMPACT:')]


def run_lint(command, timeout=None):
    """Runs the contract lint, echoes its report and surfaces COMPACT markers; a lint failure still fails."""
    done = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, encoding='utf-8', timeout=timeout)
    print(done.stdout + done.stderr, end='', flush=True)
    for warning in compact_warnings(done.stdout, bool(os.environ.get('GITHUB_ACTIONS'))):
        print(warning, flush=True)
    if done.returncode:
        raise subprocess.CalledProcessError(done.returncode, command)


def format_commands(tracked):
    """The format gate. src/ reaches the crate only through generated instrumentation, so cargo fmt never sees it."""
    rust = [p for p in tracked if p.endswith('.rs') and not p.startswith('tests/fixtures/')]
    return [['rustup', 'component', 'add', 'rustfmt', '--toolchain', 'stable'],
            ['rustfmt', '+stable', '--check', *rust],
            ['npm', 'run', '-s', 'format:check']]


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
    # Windows process fixtures spawn peers inside each harness case. Bound oversubscription,
    # preserving their own concurrency and wall-clock deadlines and an explicit runner override.
    if sys.platform == 'win32':
        env.setdefault('RUST_TEST_THREADS', str(min(4, os.cpu_count() or 1)))
    for key in ('TASKQ_TASK', 'TASKQ_ATTEMPT', 'TASKQ_ROLE', 'TASKQ_ROOT', 'CODEX_THREAD_ID', 'CODEX_SESSION_ID',
                'CLAUDE_CODE_SESSION_ID', 'COREQ_TRACE_CONTEXT', 'GH_TOKEN', 'GITHUB_TOKEN', 'GITLAB_TOKEN', 'TASKQ_INCIDENT_REPORTS'):
        env.pop(key, None)
    return {**env, 'TASKQ_INSTALL': str(install)}


def tree_sha(root=ROOT):
    """The tree of the working files, uncommitted changes included; the real index stays untouched. None when unknown."""
    with tempfile.TemporaryDirectory(prefix='taskq-check-index-') as stage:
        env = {**os.environ, 'GIT_INDEX_FILE': str(Path(stage) / 'index')}
        git = lambda *a: subprocess.run(['git', *a], cwd=root, env=env, capture_output=True, text=True, check=True).stdout.strip()
        try:
            git('read-tree', 'HEAD')
            git('add', '-A')
            return git('write-tree')
        except (OSError, subprocess.SubprocessError):
            return None


def receipt_path(environment, root=ROOT):
    """docs/testing.md: a local full gate's receipt lives in the shared Git directory, so a reviewer's worktree sees a worker's. CI keeps none."""
    tree = None if environment.get('GITHUB_ACTIONS') else tree_sha(root)
    if not tree:
        return None
    common = subprocess.check_output(['git', 'rev-parse', '--git-common-dir'], cwd=root, text=True).strip()
    return (Path(root) / common).resolve() / 'taskq-check-receipts' / f'{tree}.json'


def repeat_refusal(path, reason):
    """The refusal of a repeat full gate on one tree; a `--reason` run proceeds."""
    if reason or path is None or not path.is_file():
        return None
    try:
        prior = json.loads(path.read_text(encoding='utf-8'))
        ran = f"{prior['result']} at {prior['at']} on {prior['head']}"
    except (OSError, ValueError, TypeError, KeyError):
        ran = 'with an unreadable receipt'
    return f'full gate already ran on this tree, {ran} ({path}). Read its result or exact-SHA CI, or name why with --reason (docs/testing.md).'


def write_receipt(path, record):
    """A receipt saves work and proves nothing: a failed write warns and never changes the gate's outcome."""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile('w', encoding='utf-8', dir=path.parent, delete=False) as stage:
            json.dump(record, stage)
        os.replace(stage.name, path)
        print(f"Check receipt: {record['result']} {path}", flush=True)
    except OSError as error:
        print(f'Warning: no check receipt: {error}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', default=os.environ.get('CHECK_BASE', 'HEAD^'))
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--reason', default='', help='why this tree needs another full gate (docs/testing.md)')
    parser.add_argument('--plan', action='store_true', help='print selection without running checks')
    parser.add_argument('--rules', action='store_true', help='run only the rule-change gate and coverage report')
    parser.add_argument('--lint', action='store_true', help='prepare and run only the pinned specq contract lint')
    args = parser.parse_args()
    if args.lint:
        command = contract_lint()
        run_lint(command)
        return
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
    reason = args.reason.strip()
    if args.reason and not reason:
        raise SystemExit('--reason needs a named reason (docs/testing.md)')
    # Selected runs stay cheap and keep the lint first: only a full gate reads the tree here.
    receipt = receipt_path(os.environ) if scope == 'full' else None
    refused = repeat_refusal(receipt, reason)
    if refused:
        raise SystemExit(refused)
    if scope == 'full' and receipt is None and not os.environ.get('GITHUB_ACTIONS'):
        print('Warning: no check receipt: the tree is unknown', flush=True)
    print('Selected checks:', scope, '| base:', args.base, flush=True)
    started = time.monotonic()
    result = 'failed'
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
        # CI runs lint once in the plan job; local failures stop before tests or builds.
        if not os.environ.get('GITHUB_ACTIONS'):
            lint = contract_lint()
            print('+', ' '.join(lint), flush=True)
            run_lint(lint, BUDGET - (time.monotonic() - started))
        run(['git', 'diff', '--check', args.base if paths else 'HEAD'])
        tracked = subprocess.check_output(['git', 'ls-files'], cwd=ROOT, text=True, encoding='utf-8').splitlines()
        if tree_evidence(tracked):
            raise SystemExit('evidence belongs outside the tree (docs/testing.md#visual-evidence):\n' + '\n'.join(tree_evidence(tracked)))
        run([sys.executable, 'scripts/check.test.py'])
        run([sys.executable, 'scripts/batch-replay.test.py'])
        run([sys.executable, 'scripts/live-probe.test.py'])
        run([sys.executable, 'scripts/telemetry.test.py'])
        # CI runs the rule gate once, in the plan job; local runs include it.
        failed = None if os.environ.get('GITHUB_ACTIONS') else rule_gate(args.base)
        if failed:
            raise SystemExit(failed)
        found = [f for p in sorted(ROOT.glob('src/**/*.rs')) for f in runtime_names(p.relative_to(ROOT).as_posix(), p.read_text(encoding='utf-8'))]
        if found:
            raise SystemExit('runtime name outside the data table or runtime-name branch (docs/runtime-qualification.md#runtime-adapters):\n' + '\n'.join(found))
        if scope != 'docs':
            run(['npm', 'ci', '--ignore-scripts', '--no-audit', '--no-fund'])
            run(['npm', 'run', 'build'])
            # Linux only: a Windows checkout may hold CRLF, which format:check refuses.
            if sys.platform == 'linux':
                for command in format_commands(tracked):
                    run(command)
            # npm ci removes the documented no-save browser instrument dependency.
            if os.environ.get('RUNNER_OS') is None and any(p.startswith('tests/') and p.endswith('.browser.cjs') for p in paths):
                run(['npm', 'install', '--no-save', '--package-lock=false', '--ignore-scripts', '--no-audit', '--no-fund', 'playwright@1.64.1'])
            with tempfile.TemporaryDirectory(prefix='taskq-tests-') as install:
                test_env = test_environment(env, install)
                print('Rust harness threads:', test_env.get('RUST_TEST_THREADS', 'runtime default'), flush=True)
                run(['cargo', '+stable', 'test', '--locked'] + ([] if scope == 'full' else ['--bin', 'taskq', 'core::tests']), environment=test_env)
            if scope == 'full':
                with tempfile.TemporaryDirectory(prefix='taskq-gate-') as install:
                    run(['cargo', '+stable', 'run', '--locked', '--', 'selftest'], {'TASKQ_SELFTEST_OFFLINE':'1', 'TASKQ_INCIDENT_REPORTS':'off'}, environment=test_environment(env, install))
            if os.environ.get('RUNNER_OS') in (None, 'Linux'):
                for script in sorted((ROOT / 'tests').glob('*.test.cjs')):
                    run(['node', script.relative_to(ROOT).as_posix()])
                run(['sh', 'docs/shots/check.test.sh'])
                run(['sh', 'docs/shots/update.test.sh'])
                run(['sh', '.github/release-tag.test.sh'])
                run(['sh', '.github/release-publish.test.sh'])
                run([sys.executable, '.github/release-check.test.py'])
        for path in paths:
            if path.startswith('tests/') and path.endswith('.browser.cjs') and os.environ.get('RUNNER_OS') is None:
                run(['node', path])
        result = 'passed'
    finally:
        print(f'Selected gate elapsed: {time.monotonic() - started:.3f}s / {BUDGET}s; no retries', flush=True)
        if receipt:
            head = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
            write_receipt(receipt, {'result': result, 'base': args.base, 'head': head, 'reason': reason,
                                    'at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), 'elapsed_s': round(time.monotonic() - started, 3)})


if __name__ == '__main__':
    main()
