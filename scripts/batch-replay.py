#!/usr/bin/env python3
"""Isolated #2395 experiment. No board writes, remotes, publication or runtime qualification."""
import argparse
import hashlib
import itertools
import inspect
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time


class Refusal(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise Refusal(reason)


def digest(body):
    return hashlib.sha256(body).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True) + '\n').encode()


def store(root, body):
    """Exclusive create and byte readback; even a corrupt existing digest refuses."""
    root.mkdir(parents=True, exist_ok=True)
    key = digest(body)
    path = root / key
    try:
        with path.open('xb') as stream:
            stream.write(body)
    except FileExistsError:
        pass
    require(path.read_bytes() == body, 'immutable object differs: ' + key)
    return key


def git(repo, *args, data=None):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('GIT_', 'TASKQ_', 'COREQ_')) and k not in
           {'GH_TOKEN', 'GITHUB_TOKEN', 'GITLAB_TOKEN', 'CODEX_THREAD_ID', 'CLAUDE_CODE_SESSION_ID'}}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
               GIT_AUTHOR_NAME='Replay pilot', GIT_AUTHOR_EMAIL='pilot@example.invalid',
               GIT_COMMITTER_NAME='Replay pilot', GIT_COMMITTER_EMAIL='pilot@example.invalid',
               GIT_AUTHOR_DATE='2000-01-01T00:00:00Z', GIT_COMMITTER_DATE='2000-01-01T00:00:00Z')
    done = subprocess.run(['git', '-c', 'core.autocrlf=false', '-c', 'core.hooksPath=' + os.devnull,
                           '-C', str(repo), *args], input=data, capture_output=True, env=env)
    require(done.returncode == 0, 'Git refused ' + args[0] + ': ' + done.stderr.decode(errors='replace'))
    return done.stdout


def oid(repo, revision):
    require(bool(re.fullmatch('[0-9a-f]{40}', revision)), 'full immutable Git SHA required')
    return git(repo, 'rev-parse', '--verify', revision + '^{commit}').decode().strip()


def delta(repo, base, head):
    return git(repo, 'diff', '--binary', '--full-index', '--no-renames', '--no-ext-diff', '--no-textconv', base, head, '--')


def paths(repo, base, head):
    return set(filter(None, git(repo, 'diff', '--name-only', '-z', '--no-renames', base, head, '--').split(b'\0')))


def entries(repo, tree, names):
    # Filter exact byte paths: ls-tree path arguments do not support pathspec magic.
    return b'\0'.join(row for row in git(repo, 'ls-tree', '-r', '-z', tree).split(b'\0')
                      if row and row.split(b'\t', 1)[1] in names)


def gate_identity(repo, tree):
    # Rust embeds checks in src; differing bytes cannot prove retained failing assertions.
    rows = git(repo, 'ls-tree', '-r', '-z', tree).split(b'\0')
    definitions = []
    for row in rows:
        if not row:
            continue
        path = row.split(b'\t', 1)[1]
        if path.startswith((b'src/', b'tests/', b'scripts/', b'.github/', b'docs/shots/')) or path in {
                b'Cargo.toml', b'Cargo.lock', b'build.rs', b'package.json', b'package-lock.json', b'taskq.md'}:
            definitions.append(row)
    return digest(b'\0'.join(definitions))


def verify(repo, manifest):
    require(manifest['schema'] == 1 and isinstance(manifest['synthetic'], bool), 'unsupported pilot schema')
    require(re.fullmatch('[0-9a-f]{40}', manifest['producer_revision']) is not None, 'producer revision missing')
    require(re.fullmatch('[0-9a-f]{64}', manifest.get('producer_executable_sha256', '')) is not None,
            'producer executable digest missing')
    frozen = oid(repo, manifest['frozen_head'])
    members = manifest['members']
    require(3 <= len(members) <= 5, 'pilot requires 3-5 members')
    require(len({m['task'] for m in members}) == len(members), 'duplicate task')
    occupied, verified = set(), []
    for m in members:
        # This typed projection is produced by #2391. Never parse lifecycle comments here.
        export = m['source_export']
        require(digest(encoded(export)) == m['source_export_sha256'], 'source export tampered')
        source = export['source_readiness']
        require(source['schema'] == 1 and source['accepted_result_state'] == 'proven',
                'authoritative accepted-result proof unknown for task ' + str(m['task']))
        e = source['evidence']
        require(e is not None and e['schema'] == 1, 'unsupported authoritative evidence schema')
        for key in ('task', 'attempt', 'result_entry', 'result_sha'):
            require(source[key] == e[key] == m[key], 'stale source ' + key)
        require(export['result'] == source['current_result_sha'] == m['result_sha'], 'export result moved')
        require(export.get('state') == 'closed', 'task not accepted and closed')
        require(m['dependencies'] == [] and m['dependency_evidence']['dependencies'] == [], 'dependencies refuse replay')
        require(m['dependency_evidence']['task'] == m['task'], 'dependency evidence mismatch')
        # Dependency evidence is a frozen public issue body, not a caller assertion.
        body = m['dependency_evidence']['body']
        require(not re.search(r'(?im)^\s*Depends on\b', body), 'dependency in issue body')
        f = e.get('frozen') or m['publication']
        landed = e.get('landed') or m['publication']
        require(f['repository'] == manifest['repository'] == landed['repository'], 'repository mismatch')
        require(f['pr'] == m['pr'] == landed['pr'], 'PR mismatch')
        require(source['url'] == e['url'] == 'https://github.com/' + manifest['repository'] + '/issues/' + str(m['task']),
                'task URL mismatch')
        require(f['head'] == landed['head'] == m['result_sha'], 'publication head moved')
        original = f['base']
        if original is not None:
            oid(repo, original)
        parent, merged = oid(repo, landed['parent']), oid(repo, landed['merged'])
        require(landed['integration'] == merged, 'merge relation unknown')
        require(git(repo, 'rev-parse', merged + '^').decode().strip() == parent, 'merge parent mismatch')
        oid(repo, m['result_sha'])
        patch = delta(repo, parent, merged)
        source_base = m.get('source_delta_base', original)
        require(source_base is not None, 'source delta base unknown; replay base cannot replace it')
        oid(repo, source_base)
        require(git(repo, 'merge-base', parent, m['result_sha']).decode().strip() == source_base,
                'source delta ancestor mismatch')
        source_patch = delta(repo, source_base, m['result_sha'])
        require(patch and source_patch, 'empty source delta')
        with tempfile.TemporaryDirectory(prefix='taskq-source-coverage-') as temp:
            private = Path(temp)
            parent_tree = import_tree(repo, private, parent)
            git(private, 'read-tree', parent_tree)
            git(private, 'apply', '--cached', '--binary', data=source_patch)
            require(git(private, 'write-tree').strip() == git(repo, 'rev-parse', merged + '^{tree}').strip(),
                    'hidden extra or missing source changes')
        require(digest(patch) == m['delta_sha256'], 'source delta tampered')
        ci = m['ci']
        require(ci['run_id'] > 0 and ci['run_attempt'] > 0 and ci['conclusion'] == 'success', 'CI run/attempt proof missing')
        expected_ci = merged if landed['ci_kind'] == 'merge_group' else m['result_sha']
        require(landed['ci_kind'] in ('merge_group', 'private_exact_sha') and
                ci['event'] == landed['ci_kind'] and ci['head_sha'] == landed['ci_sha'] == expected_ci,
                'wrong exact-source CI')
        observation = ci['observation']
        require(ci['run_id'] == observation['id'] and ci['run_attempt'] == observation['run_attempt'] and
                ci['head_sha'] == observation['head_sha'] and observation['status'] == 'completed' and
                observation['event'] == ci['event'] and observation['conclusion'] == ci['conclusion'],
                'CI run/attempt differs from frozen API readback')
        names = paths(repo, parent, merged)
        require(not names & occupied, 'overlapping task deltas refuse independence')
        occupied |= names
        verified.append({**m, 'patch': patch, 'paths': names, 'parent': parent, 'merged': merged,
                         'source_patch': source_patch, 'original_pr_base': original})
    return frozen, verified


def import_tree(source, target, head):
    """Copy only objects reachable from the frozen tree, never historical branches."""
    tree = git(source, 'rev-parse', head + '^{tree}').strip()
    objects = git(source, 'rev-list', '--objects', tree.decode())
    ids = b'\n'.join(line.split(b' ', 1)[0] for line in objects.splitlines()) + b'\n'
    pack = git(source, 'pack-objects', '--stdout', data=ids)
    git(target, 'init', '--quiet')
    git(target, 'unpack-objects', '-q', data=pack)
    return tree.decode()


def apply(repo, member):
    # Exact per-path blob/mode checks disallow fuzzy application to moved inputs.
    before = git(repo, 'write-tree').decode().strip()
    require(entries(repo, before, member['paths']) == member['before'],
            'replay input changed for task ' + str(member['task']))
    output = git(repo, 'apply', '--cached', '--binary', data=member['patch'])
    after = git(repo, 'write-tree').decode().strip()
    require(entries(repo, after, member['paths']) == member['after'], 'replay output changed')
    return {'task': member['task'], 'argv': ['git', 'apply', '--cached', '--binary'],
            'stdin_sha256': digest(member['patch']), 'before_tree': before,
            'after_tree': after, 'stdout': output.decode(), 'exit_code': 0}


def prepare(source, target, head, members, receipts=None, identity=None):
    tree = import_tree(source, target, head)
    # Retain this exact reviewed commit as the bundle boundary, never a task branch.
    body = git(source, 'cat-file', 'commit', head)
    require(git(target, 'hash-object', '-t', 'commit', '-w', '--stdin', data=body).decode().strip() == head,
            'reviewed source commit changed')
    (target / '.git' / 'shallow').write_text(head + '\n', encoding='ascii')
    git(target, 'read-tree', tree)
    for m in reversed(members):
        m['after'] = entries(target, git(target, 'write-tree').decode().strip(), m['paths'])
        git(target, 'apply', '--cached', '--binary', '--reverse', data=m['patch'])
        m['before'] = entries(target, git(target, 'write-tree').decode().strip(), m['paths'])
    base = git(target, 'write-tree').decode().strip()
    base_commit = git(target, 'commit-tree', base, '-p', head, data=b'Private experimental base\n').decode().strip()
    orders = []
    for order in itertools.permutations(members):
        started = time.monotonic()
        began_at = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
        steps, error, actual = [], None, None
        try:
            git(target, 'read-tree', base)
            for m in order:
                steps.append(apply(target, m))
            actual = git(target, 'write-tree').decode().strip()
            require(actual == tree, 'replay order changed frozen tree')
        except Refusal as refused:
            error = str(refused)
        record = {**(identity or {}), 'schema': 1, 'tasks': [m['task'] for m in order],
                  'frozen_source_head': head, 'experimental_base_head': base_commit,
                  'experimental_base_tree': base, 'expected_tree': tree, 'tree': actual,
                  'outcome': 'failed' if error else 'passed', 'started_utc': began_at,
                  'elapsed_seconds': time.monotonic() - started}
        if receipts is not None:
            log = {'reset_argv': ['git', 'read-tree', base], 'steps': steps, 'error': error,
                   'cwd': 'private temporary Git repository', 'git_wrapper': 'defined by tool digest'}
            record['log_sha256'] = store(receipts, encoded(log))
            record['receipt'] = store(receipts, encoded(record))
        orders.append(record)
        require(error is None, error)
    return base_commit, tree, orders


def checkout(repo, base, members):
    git(repo, 'read-tree', base)
    for member in members:
        apply(repo, member)
    tree = git(repo, 'write-tree').decode().strip()
    trailers = ['Task: ' + m['source_export']['source_readiness']['url'] for m in members]
    message = 'Isolated replay; no publication\n\n' + '\n'.join(trailers) + '\n'
    commit = git(repo, 'commit-tree', tree, '-p', base, data=message.encode()).decode().strip()
    git(repo, 'clean', '-fdx')
    git(repo, 'reset', '--hard', '--quiet', commit)
    return commit, tree


def localize(members, check):
    """All proper subsets including baseline; unknown checks cannot prove minimality."""
    outcomes = []
    for size in range(len(members)):
        for subset in itertools.combinations(members, size):
            outcomes.append(check(subset))
    failures = [r for r in outcomes if r['outcome'] == 'failed']
    minimal = [r['tasks'] for r in failures if all(s['outcome'] == 'passed' for s in outcomes
                                                if set(s['tasks']) < set(r['tasks']))]
    unresolved = [r['tasks'] for r in failures if r['tasks'] not in minimal and
                  not any(set(s['tasks']) < set(r['tasks']) for s in failures)]
    unknown = [r['tasks'] for r in outcomes if r['outcome'] == 'unknown']
    interaction = any(len(s) > 1 for s in minimal) or (not failures and not unknown)
    return {'subsets': outcomes, 'minimal_failures': minimal,
            'unresolved_failures': unresolved,
            'baseline_failed': [] in minimal,
            'interaction': True if interaction else None if unknown else False,
            'unknown': unknown}


def execute(source, manifest, receipts, runner=None, prepare_only=False):
    started = time.monotonic()
    input_id = store(receipts, encoded(manifest))
    tool_id = store(receipts, Path(__file__).read_bytes())
    require(runner is None or manifest['synthetic'], 'fixture runner cannot qualify real tasks')
    identity = {'input': input_id, 'tool': tool_id, 'host': platform.node(),
                'synthetic': manifest['synthetic'],
                'platform': platform.platform(), 'python': sys.version,
                'git': git(source, '--version').decode().strip()}
    if runner is not None:
        identity['synthetic_gate_tool'] = store(receipts, inspect.getsource(runner).encode())
    if runner is None:
        versions = {}
        for name, argv in [('cargo', ['cargo', '+stable', '--version']),
                           ('rustc', ['rustc', '+stable', '--version']),
                           ('node', ['node', '--version']), ('npm', ['npm', '--version'])]:
            executable = shutil.which(argv[0])
            require(executable is not None, 'gate tool unavailable: ' + name)
            done = subprocess.run([executable, *argv[1:]], capture_output=True, timeout=30)
            require(done.returncode == 0, 'gate tool version unknown: ' + name)
            versions[name] = done.stdout.decode(errors='replace').strip()
        identity['gate_tool_versions'] = versions
    identity['environment_sha256'] = digest(encoded(dict(os.environ)))
    frozen, members = verify(source, manifest)
    with tempfile.TemporaryDirectory(prefix='taskq-private-replay-') as temp:
        repo = Path(temp)
        base, tree, orders = prepare(source, repo, frozen, members, receipts, identity)
        original_gate = gate_identity(repo, tree)
        deltas = [{'task': m['task'], 'original_pr_base': m['original_pr_base'],
                   'original_merge_parent': m['parent'], 'merged': m['merged'],
                   'patch': store(receipts, m['patch']), 'source_patch': store(receipts, m['source_patch'])}
                  for m in members]
        replay_id = store(receipts, encoded({**identity, 'frozen_source_head': frozen,
                            'experimental_base_head': base, 'expected_tree': tree, 'orders': orders,
                            'deltas': deltas, 'original_gate_identity': original_gate,
                            'replay_command': ['git', 'apply', '--cached', '--binary']}))
        command = (getattr(runner, 'command', ['synthetic callback', identity['synthetic_gate_tool']])
                   if runner is not None else [sys.executable, 'scripts/check.py', '--full'])
        if prepare_only:
            head, replay_tree = checkout(repo, base, members)
            require(replay_tree == tree, 'prepared head differs from reviewed source tree')
            git(repo, 'update-ref', 'refs/heads/pilot', head)
            bundle = repo / '.git' / 'pilot.bundle'
            git(repo, 'bundle', 'create', str(bundle), 'refs/heads/pilot', '^' + frozen)
            bundle_log = git(source, 'bundle', 'verify', str(bundle))
            public = {'schema': 1, 'synthetic': manifest['synthetic'], 'outcome': 'pending',
                      'frozen_source_head': frozen, 'expected_tree': tree, 'replay_head': head,
                      'experimental_base_head': base, 'replay_tree': replay_tree,
                      'input_sha256': input_id, 'pilot_tool_sha256': tool_id,
                      'original_gate_identity': original_gate, 'replay_receipt': replay_id,
                      'producer_revision': manifest['producer_revision'],
                      'producer_executable_sha256': manifest['producer_executable_sha256'],
                      'bundle_sha256': store(receipts, bundle.read_bytes()),
                      'gate_command': ['python3', 'scripts/check.py', '--full'],
                      'members': [{k: m[k] for k in ('task', 'pr', 'attempt', 'result_entry', 'result_sha',
                                                   'original_pr_base', 'ci')} for m in members],
                      'publication': 'none', 'runtime_qualification': 'none'}
            public['manifest_sha256'] = store(receipts, encoded(public))
            record = {**identity, 'prepared_manifest': public, 'outcome': 'pending',
                      'elapsed_seconds': time.monotonic() - started,
                      'log_sha256': store(receipts, bundle_log),
                      'next': 'operator exact-SHA full-matrix proof required'}
            record['receipt'] = store(receipts, encoded(record))
            return record
        cache = {}

        def check(subset):
            head, subset_tree = checkout(repo, base, subset)
            actual_gate = gate_identity(repo, subset_tree)
            key = digest(encoded({**identity, 'command': command, 'head': head,
                                  'actual_gate_identity': actual_gate, 'original_gate_identity': original_gate,
                                  'tasks': [m['task'] for m in subset]}))
            if key in cache:
                return cache[key]
            cache_path = receipts / 'cache' / key
            if cache_path.exists():
                receipt_id = cache_path.read_text(encoding='ascii')
                require(re.fullmatch('[0-9a-f]{64}', receipt_id) is not None, 'cache receipt identity tampered')
                body = (receipts / receipt_id).read_bytes()
                require(digest(body) == receipt_id, 'cached receipt tampered')
                record = json.loads(body)
                require(record['cache_key'] == key, 'cache input mismatch')
                log = (receipts / record['log_sha256']).read_bytes()
                require(digest(log) == record['log_sha256'], 'cached log tampered')
                record['receipt'] = receipt_id
                cache[key] = record
                return record
            begin = time.monotonic()
            began_at = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
            if runner is not None:
                code, log = runner(repo, subset)
            else:
                # Canonical runner already isolates fixture installs. Remove worker credentials/context.
                env = {k: v for k, v in os.environ.items() if not k.startswith(('TASKQ_', 'COREQ_', 'GIT_'))
                       and k not in {'GH_TOKEN', 'GITHUB_TOKEN', 'GITLAB_TOKEN', 'CODEX_THREAD_ID',
                                    'CLAUDE_CODE_SESSION_ID', 'GITHUB_ACTIONS', 'CHECK_BASE', 'CHECK_FULL', 'RUNNER_OS'}}
                with tempfile.TemporaryDirectory(prefix='taskq-pilot-install-') as install:
                    env.update(TASKQ_INSTALL=install, TASKQ_SELFTEST_OFFLINE='1', TASKQ_INCIDENT_REPORTS='off')
                    try:
                        done = subprocess.run(command, cwd=repo, env=env, capture_output=True, timeout=930)
                        code, log = done.returncode, done.stdout + done.stderr
                    except subprocess.TimeoutExpired as error:
                        code, log = None, (error.stdout or b'') + (error.stderr or b'') + b'\nPilot outer deadline exceeded\n'
            if git(repo, 'rev-parse', 'HEAD').decode().strip() != head or git(repo, 'status', '--porcelain'):
                code, log = None, log + b'\nGate changed frozen inputs; qualification refused\n'
            record = {**identity, 'replay': replay_id, 'cache_key': key, 'command': command,
                      'tasks': [m['task'] for m in subset], 'gate_head': head, 'gate_tree': subset_tree,
                      'observed_outcome': 'unknown' if code is None else 'passed' if code == 0 else 'failed',
                      'outcome': 'unknown' if code is None or actual_gate != original_gate else 'passed' if code == 0 else 'failed',
                      'actual_gate_identity': actual_gate, 'original_gate_identity': original_gate,
                      'comparability': 'same check inputs' if actual_gate == original_gate else 'unknown: check inputs changed',
                      'exit_code': code, 'elapsed_seconds': time.monotonic() - begin,
                      'started_utc': began_at,
                      'log_sha256': store(receipts, log)}
            record['receipt'] = store(receipts, encoded(record))
            cache_path.parent.mkdir(exist_ok=True)
            try:
                with cache_path.open('x', encoding='ascii') as stream:
                    stream.write(record['receipt'])
            except FileExistsError:
                require(cache_path.read_text(encoding='ascii') == record['receipt'], 'cache changed concurrently')
            cache[key] = record
            return record

        original = check(members)
        diagnostics = localize(members, check) if original['outcome'] == 'failed' else None
        result = {**identity, 'synthetic': manifest['synthetic'], 'replay': replay_id,
                  'original_batch': original, 'diagnostics': diagnostics,
                  'outcome': original['outcome'], 'elapsed_seconds': time.monotonic() - started,
                  'publication': 'none', 'runtime_qualification': 'none'}
        result['receipt'] = store(receipts, encoded(result))
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--receipts', type=Path, required=True)
    parser.add_argument('--prepare-bundle', action='store_true', help='prepare offline operator proof; never qualifies the gate')
    args = parser.parse_args()
    started = time.monotonic()
    manifest_bytes = None
    try:
        manifest_bytes = args.manifest.read_bytes()
        result = execute(args.source.resolve(), json.loads(manifest_bytes), args.receipts.resolve(),
                         prepare_only=args.prepare_bundle)
    except (Refusal, KeyError, TypeError, AttributeError, OSError, UnicodeError,
            json.JSONDecodeError, subprocess.TimeoutExpired) as error:
        result = {'outcome': 'unknown', 'reason': str(error), 'qualification': 'refused',
                  'input_bytes': store(args.receipts.resolve(), manifest_bytes) if manifest_bytes is not None else None,
                  'tool': store(args.receipts.resolve(), Path(__file__).read_bytes()),
                  'host': platform.node(), 'elapsed_seconds': time.monotonic() - started}
        result['receipt'] = store(args.receipts.resolve(), encoded(result))
    print(json.dumps(result, sort_keys=True))
    return 0 if result['outcome'] in ('passed', 'pending') else 1


if __name__ == '__main__':
    sys.exit(main())
