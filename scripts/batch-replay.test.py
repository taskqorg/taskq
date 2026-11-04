"""R8/R12/R13; synthetic only. Prevent unsupported or changed inputs qualifying a replay."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('batch_replay', Path(__file__).with_name('batch-replay.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)

INPUT_VARIABLE = 'REPLAY_FIXTURE_INPUT'
# Invented canonical gate: passes unless a file outside every Git tree says otherwise.
GATE_STUB = ("import os, sys\n"
             "path = os.environ.get('" + INPUT_VARIABLE + "')\n"
             "sys.exit(0 if not path or open(path).read() == 'ok' else 1)\n")


def one_order(members):
    # Only the end-to-end test and the later-hunk test need every permutation.
    return [tuple(members)]


def no_gate(repo, subset):
    raise AssertionError('prepare only')


class Replay(unittest.TestCase):
    """Shares one immutable source and one prepared replay; every test writes only its own copies."""

    @classmethod
    def setUpClass(cls):
        cls.shared = tempfile.TemporaryDirectory(prefix='taskq-synthetic-replay-')
        cls.addClassCleanup(cls.shared.cleanup)
        shared = Path(cls.shared.name)
        cls.repo = shared / 'source'
        cls.repo.mkdir()
        b.git(cls.repo, 'init', '--quiet', '--template=')
        (cls.repo / 'anchor').write_text('invented fixture\n')
        (cls.repo / 'scripts').mkdir()
        (cls.repo / 'scripts' / 'check.py').write_text(GATE_STUB)
        b.git(cls.repo, 'add', '.')
        b.git(cls.repo, 'commit', '-qm', 'Synthetic base')
        cls.fixture = {'schema': 1, 'synthetic': True, 'producer_revision': 'a' * 40,
                       'producer_executable_sha256': 'b' * 64,
                       'repository': 'invented/repo', 'members': []}
        cls.base = cls.rev(cls.repo)
        parent = cls.base
        for n in range(1, 6):
            (cls.repo / ('member-' + str(n))).write_text('invented task ' + str(n) + '\n')
            b.git(cls.repo, 'add', '.')
            b.git(cls.repo, 'commit', '-qm', 'Synthetic member ' + str(n))
            head = cls.rev(cls.repo)
            pub = {'repository': 'invented/repo', 'pr': n, 'base': parent, 'head': head,
                   'parent': parent, 'merged': head, 'integration': head, 'ci_kind': 'merge_group',
                   'ci_sha': head, 'ci_run_id': n, 'ci_run_attempt': 1}
            e = {'schema': 1, 'task': n, 'attempt': 10 * n, 'result_entry': 10 * n + 1,
                 'result_sha': head, 'url': 'https://github.com/invented/repo/issues/' + str(n),
                 'frozen': dict(pub), 'landed': dict(pub)}
            export = {'task': n, 'state': 'closed', 'result': head,
                      'source_readiness': {'schema': 1, 'state': 'source_ready', 'accepted_result_state': 'proven',
                                           'current_result_sha': head, 'evidence': e,
                                           **{key: e[key] for key in ('task', 'url', 'attempt', 'result_entry', 'result_sha')}}}
            cls.fixture['members'].append({'task': n, 'attempt': e['attempt'],
                'result_entry': e['result_entry'], 'result_sha': head, 'pr': n,
                'source_export': export, 'source_export_sha256': b.digest(b.encoded(export)),
                'dependencies': [], 'dependency_evidence': {'task': n, 'dependencies': [],
                'body': '## Goal\nInvented independent fixture'},
                'delta_sha256': b.digest(b.delta(cls.repo, parent, head)),
                'ci': {'run_id': n, 'run_attempt': 1, 'conclusion': 'success',
                       'head_sha': head, 'event': 'merge_group',
                       'observation': {'id': n, 'run_attempt': 1, 'status': 'completed',
                                       'head_sha': head, 'event': 'merge_group', 'conclusion': 'success'}}})
            parent = head
        cls.fixture['frozen_head'] = cls.fixture_head = cls.rev(cls.repo)
        # Proofs are pure over immutable commits; a test that changes a member misses this memo.
        cls.proofs = {}
        cls.real_verify_member = staticmethod(b.verify_member)
        cls.prepared_repo = shared / 'prepared'
        cls.prepared_repo.mkdir()
        manifest = copy.deepcopy(cls.fixture)
        manifest['members'] = manifest['members'][:3]
        identity = {'input': 'shared', 'tool': 'shared'}
        with mock.patch.object(b, 'verify_member', cls.proof):
            cls.prepared = b.replay(cls.repo, manifest, shared / 'prepared-receipts', identity,
                                    cls.prepared_repo, orders=one_order)

    @classmethod
    def tearDownClass(cls):
        # Reuse is only safe while no test wrote to the shared source.
        assert cls.rev(cls.repo) == cls.fixture_head, 'shared source moved'
        assert b.git(cls.repo, 'status', '--porcelain') == b'', 'shared source changed'

    @classmethod
    def proof(cls, repo, repository, member):
        key = b.digest(b.encoded(member))
        if key not in cls.proofs:
            cls.proofs[key] = cls.real_verify_member(repo, repository, member)
        return dict(cls.proofs[key])

    @staticmethod
    def rev(repo):
        return b.git(repo, 'rev-parse', 'HEAD').decode().strip()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='taskq-synthetic-case-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def manifest(self, count=3):
        m = copy.deepcopy(self.fixture)
        m['members'] = m['members'][:count]
        return m

    def writable_source(self):
        # A byte copy, never hardlinks: a test may corrupt or commit in its own repository.
        target = self.root / 'writable-source'
        shutil.copytree(self.repo, target)
        return target

    def stage(self, runner=None, receipts=None):
        """A private copy of the prepared replay, so its checkout state is this test's alone."""
        repo = self.root / 'replay'
        shutil.copytree(self.prepared_repo, repo)
        receipts = receipts or self.root / 'receipts'
        identity = b.identify(self.repo, self.manifest(), receipts, runner)
        return repo, receipts, identity, copy.deepcopy(self.prepared)

    def test_three_through_five_members_and_real_git_orders_and_receipts(self):
        with mock.patch.object(b, 'verify_member', self.proof):
            for count in (3, 4, 5):
                m = self.manifest(count)
                self.assertEqual(len(b.verify(self.repo, m)[1]), count)
        # Six actual Git orders protect replay. Larger cardinalities reuse that path;
        # avoid 144 redundant permutations in every canonical gate on Windows.
        m = self.manifest()
        called = []
        def runner(repo, subset):
            called.append([s['task'] for s in subset])
            self.assertEqual(b.git(repo, 'status', '--porcelain'), b'')
            self.assertEqual(b.git(repo, 'remote'), b'')
            return 0, b'SYNTHETIC gate passed; not canonical evidence\n'
        receipts = self.root / 'e2e-receipts'
        result = b.execute(self.repo, m, receipts, runner)
        self.assertTrue(result['synthetic'])
        self.assertEqual(result['outcome'], 'passed')
        replay = json.loads((receipts / result['replay']).read_bytes())
        self.assertEqual(len(replay['orders']), 6)
        for order in replay['orders']:
            self.assertEqual(order['outcome'], 'passed')
            body = (receipts / order['receipt']).read_bytes()
            self.assertEqual(b.digest(body), order['receipt'])
            log = json.loads((receipts / order['log_sha256']).read_bytes())
            self.assertEqual([s['task'] for s in log['steps']], order['tasks'])
            self.assertEqual(log['steps'][-1]['after_tree'], order['tree'])
            self.assertTrue(all(s['exit_code'] == 0 for s in log['steps']))
        self.assertEqual(called, [[1, 2, 3]])
        for path in receipts.iterdir():
            if path.is_file():
                self.assertEqual(path.name, b.digest(path.read_bytes()))
        # The next run reads the stored receipt as evidence and still executes the gate.
        repo, receipts, identity, prepared = self.stage(runner, receipts)
        repeated = b.qualify(repo, receipts, identity, prepared, runner)
        self.assertEqual(repeated['original_batch']['cache_key'], result['original_batch']['cache_key'])
        self.assertEqual(len(called), 2, 'a stored local receipt must not replace a new gate execution')
        self.assertEqual(repeated['original_batch']['prior_executions'],
                         [{'receipt': result['original_batch']['receipt'], 'observed_outcome': 'passed'}])
        log_path = receipts / result['original_batch']['log_sha256']
        log_path.write_bytes(b'corrupt cached log')
        with self.assertRaisesRegex(b.Refusal, 'stored log tampered'):
            b.qualify(repo, receipts, identity, prepared, runner)
        self.assertEqual(b.git(self.repo, 'status', '--porcelain'), b'')

    def test_external_input_change_with_same_identity_cannot_reuse_a_stored_pass(self):
        # The default runner reads a file no receipt identity covers: only re-execution sees it.
        external = self.root / 'external-input'
        external.write_text('ok')
        with mock.patch.dict(os.environ, {INPUT_VARIABLE: str(external)}), \
                mock.patch.object(b, 'tool_versions', lambda: {'fixture': 'invented'}):
            repo, receipts, identity, prepared = self.stage()
            members = prepared['members']
            passed = b.qualify(repo, receipts, identity, prepared)
            self.assertEqual(passed['outcome'], 'passed')
            external.write_text('bad')
            failed = b.qualify(repo, receipts, identity, prepared)
            original = failed['original_batch']
            self.assertEqual(original['cache_key'], passed['original_batch']['cache_key'], 'identity is unchanged')
            self.assertEqual(original['outcome'], 'failed', 'before: same persisted store returned cached PASS')
            self.assertEqual(original['tasks'], [1, 2, 3], 'every member stays in the failed original')
            self.assertEqual(failed['outcome'], 'failed')
            self.assertTrue(failed['diagnostics']['baseline_failed'])
            self.assertEqual(original['prior_executions'],
                             [{'receipt': passed['original_batch']['receipt'], 'observed_outcome': 'passed'}])
            fresh = self.root / 'fresh-receipts'
            fresh_identity = b.identify(self.repo, self.manifest(), fresh)
            self.assertEqual(b.checker(repo, fresh, fresh_identity, prepared)(members)['outcome'], 'failed')
            # A later green run of the same identity stays unknown: no retry to green.
            external.write_text('ok')
            retried = b.checker(repo, receipts, identity, prepared)(members)
            self.assertEqual(retried['observed_outcome'], 'passed')
            self.assertEqual(retried['outcome'], 'unknown')
            self.assertIn('earlier read failed', retried['comparability'])

    def test_exact_input_runs_once_per_process_and_stored_receipts_refuse_tampering(self):
        calls = []
        def runner(repo, subset):
            calls.append([s['task'] for s in subset])
            return 0, b'SYNTHETIC gate passed; not canonical evidence\n'
        repo, receipts, identity, prepared = self.stage(runner)
        check = b.checker(repo, receipts, identity, prepared, runner)
        first = check(prepared['members'])
        self.assertIs(check(prepared['members']), first)
        self.assertEqual(calls, [[1, 2, 3]], 'exact-input cache runs one gate in one process')
        key = first['cache_key']
        marker = receipts / 'executions' / key / first['receipt']
        self.assertEqual(b.prior_executions(receipts, key),
                         [{'receipt': first['receipt'], 'observed_outcome': 'passed'}])
        body = (receipts / first['receipt']).read_bytes()
        with self.subTest('receipt bytes'):
            (receipts / first['receipt']).write_bytes(b'tampered')
            with self.assertRaisesRegex(b.Refusal, 'stored receipt tampered'):
                b.prior_executions(receipts, key)
            (receipts / first['receipt']).write_bytes(body)
        with self.subTest('receipt filed under another key'):
            other = receipts / 'executions' / ('0' * 64)
            other.mkdir()
            (other / first['receipt']).touch()
            with self.assertRaisesRegex(b.Refusal, 'stored receipt input mismatch'):
                b.prior_executions(receipts, '0' * 64)
        with self.subTest('receipt identity'):
            bad = marker.with_name('not-a-digest')
            bad.touch()
            with self.assertRaisesRegex(b.Refusal, 'stored receipt identity tampered'):
                b.prior_executions(receipts, key)

    def test_refuse_missing_stale_tampered_dependent_and_wrong_ci_proof(self):
        for case in ('attempt', 'result_entry', 'result_sha', 'export', 'unknown', 'base',
                     'dependency', 'body', 'ci', 'run_attempt', 'patch', 'head', 'extra'):
            with self.subTest(case=case):
                m = self.manifest()
                item = m['members'][0]
                e = item['source_export']['source_readiness']['evidence']
                if case in ('attempt', 'result_entry'):
                    item[case] += 1
                elif case == 'result_sha':
                    item['result_sha'] = 'f' * 40
                elif case == 'export':
                    item['source_export']['state'] = 'open'
                elif case == 'unknown':
                    item['source_export']['source_readiness']['accepted_result_state'] = 'unknown'
                elif case == 'base':
                    e['frozen']['base'] = None
                elif case == 'dependency':
                    item['dependencies'] = [2]
                elif case == 'body':
                    item['dependency_evidence']['body'] += '\nDepends on other#9'
                elif case == 'ci':
                    item['ci']['head_sha'] = 'f' * 40
                elif case == 'run_attempt':
                    item['ci']['run_attempt'] = 2
                elif case == 'patch':
                    item['delta_sha256'] = 'f' * 64
                elif case == 'head':
                    e['landed']['head'] = 'f' * 40
                elif case == 'extra':
                    extra = m['members'][1]['result_sha']
                    item['result_sha'] = e['result_sha'] = extra
                    item['source_export']['result'] = extra
                    item['source_export']['source_readiness'].update(current_result_sha=extra, result_sha=extra)
                    e['frozen']['head'] = e['landed']['head'] = extra
                if case != 'export':
                    item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
                with self.assertRaises(b.Refusal):
                    b.verify(self.repo, m)
        for count in (2, 6):
            m = self.manifest()
            m['members'] = (m['members'] * 2)[:count]
            with self.assertRaises(b.Refusal):
                b.verify(self.repo, m)

    def test_unknown_historical_base_stays_distinct_from_proven_replay_coverage(self):
        m = self.manifest()
        for item in m['members']:
            source = item['source_export']['source_readiness']
            e = source['evidence']
            item['publication'] = copy.deepcopy(e['landed'])
            item['publication']['base'] = None
            item['source_delta_base'] = e['landed']['parent']
            e['frozen'] = e['landed'] = None
            source.update(state='unknown')
            item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
        _, members = b.verify(self.repo, m)
        self.assertTrue(all(item['original_pr_base'] is None for item in members))

    def test_moved_input_overlap_and_git_conflict_refuse(self):
        m = self.manifest()
        source = self.writable_source()
        (source / 'member-1').write_text('changed after acceptance\n')
        b.git(source, 'add', '.')
        b.git(source, 'commit', '-qm', 'Synthetic moved input')
        m['frozen_head'] = self.rev(source)
        with mock.patch.object(b, 'verify_member', self.proof), \
                self.assertRaisesRegex(b.Refusal, 'Git refused apply'):
            frozen, moved = b.verify(source, m)
            target = self.root / 'moved'
            target.mkdir()
            b.prepare(source, target, frozen, moved)
        m = self.manifest()
        # A duplicate delta under a distinct task identity must not pass independence.
        item = copy.deepcopy(m['members'][0])
        item['task'] = 99
        e = item['source_export']['source_readiness']['evidence']
        e['task'], e['url'] = 99, 'https://github.com/invented/repo/issues/99'
        item['source_export']['source_readiness']['task'] = 99
        item['source_export']['source_readiness']['url'] = e['url']
        item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
        item['dependency_evidence']['task'] = 99
        m['members'][1] = item
        with mock.patch.object(b, 'verify_member', self.proof), \
                self.assertRaisesRegex(b.Refusal, 'overlapping'):
            b.verify(self.repo, m)
        target, _, _, prepared = self.stage(no_gate)
        members = prepared['members']
        b.git(target, 'read-tree', prepared['base'])
        members[0]['patch'] = b'not a patch\n'
        with self.assertRaisesRegex(b.Refusal, 'Git refused apply'):
            b.apply(target, members[0])

    def test_failed_original_diagnostics_keep_members_and_interactions(self):
        calls = []
        command = ['python3', '-c', 'from pathlib import Path; assert not (Path("member-1").exists() and Path("member-2").exists()), "invented interacting failure"']
        def runner(repo, subset):
            ids = [m['task'] for m in subset]
            calls.append(ids)
            # Distinct files interact at the gate, not at patch application.
            self.assertEqual((repo / 'member-1').exists(), 1 in ids)
            self.assertEqual((repo / 'member-2').exists(), 2 in ids)
            done = subprocess.run(command, cwd=repo, capture_output=True)
            return done.returncode, done.stdout + done.stderr
        runner.command = command
        repo, receipts, identity, prepared = self.stage(runner)
        result = b.qualify(repo, receipts, identity, prepared, runner)
        self.assertEqual(result['outcome'], 'failed')
        self.assertEqual(result['original_batch']['tasks'], [1, 2, 3])
        self.assertEqual(result['diagnostics']['minimal_failures'], [[1, 2]])
        self.assertTrue(result['diagnostics']['interaction'])
        self.assertEqual(len(calls), 8)
        full_only = b.localize([1, 2, 3], lambda s: {'tasks': list(s), 'outcome': 'passed'})
        self.assertTrue(full_only['interaction'])
        unknown = b.localize([1, 2, 3], lambda s: {'tasks': list(s), 'outcome': 'unknown'})
        self.assertIsNone(unknown['interaction'])
        self.assertEqual(len(unknown['unknown']), 7)
        baseline = b.localize([1, 2, 3], lambda s: {'tasks': list(s), 'outcome': 'failed'})
        self.assertTrue(baseline['baseline_failed'])

    def test_immutable_receipt_replay_and_corrupt_existing_bytes(self):
        root = self.root / 'receipts'
        key = b.store(root, b'invented receipt')
        self.assertEqual(b.store(root, b'invented receipt'), key)
        (root / key).write_bytes(b'tampered')
        with self.assertRaisesRegex(b.Refusal, 'immutable object differs'):
            b.store(root, b'invented receipt')
        self.assertEqual((root / key).read_bytes(), b'tampered')

    def test_git_checkout_preserves_content_addressed_bytes(self):
        repo = self.root / 'attributes'
        repo.mkdir()
        b.git(repo, 'init', '--quiet', '--template=')
        body = b'{"invented":true}\n'
        relative = 'docs/research/2395/receipts/' + b.digest(body)
        path = repo / relative
        path.parent.mkdir(parents=True)
        path.write_bytes(body)
        b.git(repo, '-c', 'core.autocrlf=true', 'add', relative)
        path.unlink()
        b.git(repo, '-c', 'core.autocrlf=true', 'checkout-index', '--force', relative)
        self.assertNotEqual(path.read_bytes(), body, 'before: CRLF conversion breaks the receipt digest')
        attributes = Path(__file__).resolve().parents[1] / '.gitattributes'
        (repo / '.gitattributes').write_bytes(attributes.read_bytes())
        path.write_bytes(body)
        b.git(repo, '-c', 'core.autocrlf=true', 'add', '.gitattributes', relative)
        path.unlink()
        b.git(repo, '-c', 'core.autocrlf=true', 'checkout-index', '--force', relative)
        self.assertEqual(path.read_bytes(), body, 'after: immutable evidence bypasses newline conversion')

    def test_gate_cannot_pass_after_changing_frozen_inputs(self):
        def runner(repo, subset):
            b.git(repo, 'rev-parse', 'HEAD^')
            (repo / 'anchor').write_text('gate changed tracked input\n')
            return 0, b'SYNTHETIC false success\n'
        repo, receipts, identity, prepared = self.stage(runner)
        result = b.qualify(repo, receipts, identity, prepared, runner)
        self.assertEqual(result['outcome'], 'unknown')
        self.assertIsNone(result['diagnostics'])
        self.assertEqual(result['original_batch']['tasks'], [1, 2, 3])

    def test_disappearing_regression_assertions_cannot_clear_a_member(self):
        m = self.manifest()
        source = self.writable_source()
        # Replace the last member with an independent regression-check addition.
        b.git(source, 'reset', '--hard', '--quiet', self.fixture['members'][1]['result_sha'])
        parent = self.rev(source)
        (source / 'tests').mkdir()
        (source / 'tests' / 'regression.test.py').write_text('raise AssertionError("invented failing regression")\n')
        b.git(source, 'add', '.')
        b.git(source, 'commit', '-qm', 'Synthetic regression member')
        head = self.rev(source)
        item = m['members'][2]
        item['result_sha'] = head
        item['source_export']['result'] = head
        e = item['source_export']['source_readiness']['evidence']
        e['result_sha'] = head
        item['source_export']['source_readiness'].update(current_result_sha=head, result_sha=head)
        for pub in (e['frozen'], e['landed']):
            pub.update(base=parent, parent=parent, head=head, merged=head, integration=head, ci_sha=head)
        item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
        item['delta_sha256'] = b.digest(b.delta(source, parent, head))
        item['ci']['head_sha'] = item['ci']['observation']['head_sha'] = head
        m['frozen_head'] = head
        command = ['python3', '-c', 'from pathlib import Path; import runpy; [runpy.run_path(str(p)) for p in sorted(Path("tests").glob("*.test.py"))]']
        def runner(repo, subset):
            done = subprocess.run(command, cwd=repo, capture_output=True)
            return done.returncode, done.stdout + done.stderr
        runner.command = command
        receipts = self.root / 'receipts'
        repo = self.root / 'replay'
        repo.mkdir()
        identity = b.identify(source, m, receipts, runner)
        with mock.patch.object(b, 'verify_member', self.proof):
            prepared = b.replay(source, m, receipts, identity, repo, orders=one_order)
        result = b.qualify(repo, receipts, identity, prepared, runner)
        self.assertEqual(result['outcome'], 'failed')
        subset = next(r for r in result['diagnostics']['subsets'] if r['tasks'] == [1, 2])
        self.assertEqual(subset['observed_outcome'], 'passed')
        self.assertEqual(subset['outcome'], 'unknown')
        self.assertIn('check inputs changed', subset['comparability'])
        self.assertIn([1, 2], result['diagnostics']['unknown'])
        self.assertEqual(result['diagnostics']['minimal_failures'], [])
        self.assertIn([3], result['diagnostics']['unresolved_failures'])
        self.assertIsNone(result['diagnostics']['interaction'])

    def test_prepared_bundle_requires_reviewed_source_and_stays_pending(self):
        repo, receipts, identity, prepared = self.stage(no_gate)
        m = self.manifest()
        result = b.bundle_proof(self.repo, repo, m, receipts, identity, prepared, 0)
        self.assertEqual(result['outcome'], 'pending')
        public = result['prepared_manifest']
        self.assertEqual(public['outcome'], 'pending')
        self.assertEqual(public['publication'], 'none')
        bundle = receipts / public['bundle_sha256']
        self.assertIn(m['frozen_head'].encode(), bundle.read_bytes().split(b'PACK', 1)[0])
        b.git(self.repo, 'bundle', 'verify', str(bundle))
        owner = self.root / 'operator'
        b.git(self.root, 'clone', '--quiet', '--no-hardlinks', str(self.repo), str(owner))
        b.git(owner, 'fetch', '--quiet', str(bundle), 'refs/heads/pilot:refs/heads/replay')
        self.assertEqual(b.git(owner, 'rev-parse', 'refs/heads/replay^{tree}').decode().strip(), public['expected_tree'])
        self.assertEqual(b.git(owner, 'rev-parse', 'refs/heads/replay').decode().strip(), public['replay_head'])

    def test_frozen_reviewed_later_hunks_survive_exact_reverse_and_every_replay(self):
        m = self.manifest()
        source = self.writable_source()
        b.git(source, 'reset', '--hard', '--quiet', self.base)
        lines = ['invented baseline line ' + str(n) + '\n' for n in range(80)]
        (source / 'anchor').write_text(''.join(lines))
        b.git(source, 'add', '.')
        b.git(source, 'commit', '-qm', 'Synthetic long baseline')
        for n, item in enumerate(m['members'], 1):
            parent = self.rev(source)
            if n == 1:
                lines[5] = 'invented accepted task change\n'
                (source / 'anchor').write_text(''.join(lines))
            else:
                (source / ('member-' + str(n))).write_text('invented task ' + str(n) + '\n')
            b.git(source, 'add', '.')
            b.git(source, 'commit', '-qm', 'Synthetic accepted member ' + str(n))
            head = self.rev(source)
            item['result_sha'] = item['source_export']['result'] = head
            readiness = item['source_export']['source_readiness']
            readiness.update(result_sha=head, current_result_sha=head)
            readiness['evidence']['result_sha'] = head
            for pub in (readiness['evidence']['frozen'], readiness['evidence']['landed']):
                pub.update(base=parent, parent=parent, head=head, merged=head, integration=head, ci_sha=head)
            item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
            item['delta_sha256'] = b.digest(b.delta(source, parent, head))
            item['ci']['head_sha'] = item['ci']['observation']['head_sha'] = head
        lines[60] = 'invented later reviewed change outside task hunk\n'
        (source / 'anchor').write_text(''.join(lines))
        b.git(source, 'add', '.')
        b.git(source, 'commit', '-qm', 'Synthetic reviewed later source')
        m['frozen_head'] = self.rev(source)
        result = b.execute(source, m, self.root / 'receipts', no_gate, prepare_only=True)
        self.assertEqual(result['outcome'], 'pending')
        public = result['prepared_manifest']
        self.assertEqual(public['expected_tree'], b.git(source, 'rev-parse', 'HEAD^{tree}').decode().strip())


if __name__ == '__main__':
    unittest.main()
