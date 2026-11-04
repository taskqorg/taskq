"""R8/R12/R13; synthetic only. Prevent unsupported or changed inputs qualifying a replay."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('batch_replay', Path(__file__).with_name('batch-replay.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)


class Replay(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='taskq-synthetic-replay-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / 'source'
        self.repo.mkdir()
        b.git(self.repo, 'init', '--quiet')
        (self.repo / 'anchor').write_text('invented fixture\n')
        b.git(self.repo, 'add', '.')
        b.git(self.repo, 'commit', '-qm', 'Synthetic base')
        self.fixture = {'schema': 1, 'synthetic': True, 'producer_revision': 'a' * 40,
                         'producer_executable_sha256': 'b' * 64,
                         'repository': 'invented/repo', 'members': []}
        self.base = self.head()
        for n in range(1, 6):
            parent = self.head()
            (self.repo / ('member-' + str(n))).write_text('invented task ' + str(n) + '\n')
            b.git(self.repo, 'add', '.')
            b.git(self.repo, 'commit', '-qm', 'Synthetic member ' + str(n))
            head = self.head()
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
            self.fixture['members'].append({'task': n, 'attempt': e['attempt'],
                'result_entry': e['result_entry'], 'result_sha': head, 'pr': n,
                'source_export': export, 'source_export_sha256': b.digest(b.encoded(export)),
                'dependencies': [], 'dependency_evidence': {'task': n, 'dependencies': [],
                'body': '## Goal\nInvented independent fixture'},
                'delta_sha256': b.digest(b.delta(self.repo, parent, head)),
                'ci': {'run_id': n, 'run_attempt': 1, 'conclusion': 'success',
                       'head_sha': head, 'event': 'merge_group',
                       'observation': {'id': n, 'run_attempt': 1, 'status': 'completed',
                                       'head_sha': head, 'event': 'merge_group', 'conclusion': 'success'}}})
        self.fixture['frozen_head'] = self.head()

    def head(self):
        return b.git(self.repo, 'rev-parse', 'HEAD').decode().strip()

    def manifest(self, count=3):
        m = copy.deepcopy(self.fixture)
        m['members'] = m['members'][:count]
        return m

    def test_three_through_five_members_and_real_git_orders_and_receipts(self):
        for count in (3, 4, 5):
            m = self.manifest(count)
            self.assertEqual(len(b.verify(self.repo, m)[1]), count)
        # Six actual Git orders protect replay. Larger cardinalities reuse that path;
        # avoid 144 redundant permutations in every canonical gate on Windows.
        for count in (3,):
            m = self.manifest(count)
            called = []
            def runner(repo, subset):
                called.append([s['task'] for s in subset])
                self.assertEqual(b.git(repo, 'status', '--porcelain'), b'')
                self.assertEqual(b.git(repo, 'remote'), b'')
                return 0, b'SYNTHETIC gate passed; not canonical evidence\n'
            result = b.execute(self.repo, m, self.root / 'receipts', runner)
            self.assertTrue(result['synthetic'])
            self.assertEqual(result['outcome'], 'passed')
            replay = __import__('json').loads((self.root / 'receipts' / result['replay']).read_bytes())
            self.assertEqual(len(replay['orders']), {3: 6, 4: 24, 5: 120}[count])
            self.assertEqual(called, [list(range(1, count + 1))])
            for path in (self.root / 'receipts').iterdir():
                if path.is_file():
                    self.assertEqual(path.name, b.digest(path.read_bytes()))
            repeated = b.execute(self.repo, m, self.root / 'receipts', runner)
            self.assertEqual(repeated['original_batch']['receipt'], result['original_batch']['receipt'])
            self.assertEqual(len(called), 1, 'exact input replay must use cached command receipt')
            log_path = self.root / 'receipts' / result['original_batch']['log_sha256']
            log_path.write_bytes(b'corrupt cached log')
            with self.assertRaisesRegex(b.Refusal, 'cached log tampered'):
                b.execute(self.repo, m, self.root / 'receipts', runner)
            self.assertEqual(b.git(self.repo, 'status', '--porcelain'), b'')

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
        (self.repo / 'member-1').write_text('changed after acceptance\n')
        b.git(self.repo, 'add', '.')
        b.git(self.repo, 'commit', '-qm', 'Synthetic moved input')
        m['frozen_head'] = self.head()
        with self.assertRaisesRegex(b.Refusal, 'moved'):
            b.verify(self.repo, m)
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
        with self.assertRaisesRegex(b.Refusal, 'overlapping'):
            b.verify(self.repo, m)
        frozen, members = b.verify(self.repo, self.manifest())
        target = self.root / 'private'
        target.mkdir()
        base, _, _ = b.prepare(self.repo, target, frozen, members)
        b.git(target, 'read-tree', base)
        members[0]['patch'] = b'not a patch\n'
        with self.assertRaisesRegex(b.Refusal, 'Git refused apply'):
            b.apply(target, members[0])

    def test_failed_original_diagnostics_keep_members_and_interactions(self):
        calls = []
        def runner(repo, subset):
            ids = [m['task'] for m in subset]
            calls.append(ids)
            # Distinct files interact at the gate, not at patch application.
            self.assertEqual((repo / 'member-1').exists(), 1 in ids)
            self.assertEqual((repo / 'member-2').exists(), 2 in ids)
            return (1 if {1, 2} <= set(ids) else 0), b'SYNTHETIC interaction\n'
        result = b.execute(self.repo, self.manifest(), self.root / 'receipts', runner)
        self.assertEqual(result['outcome'], 'failed')
        self.assertEqual(result['original_batch']['tasks'], [1, 2, 3])
        self.assertEqual(result['diagnostics']['minimal_failures'], [[1, 2]])
        self.assertTrue(result['diagnostics']['interaction'])
        self.assertEqual(len(calls), 8)
        full_only = b.localize([1, 2, 3], lambda s: {'tasks': list(s), 'outcome': 'passed'})
        self.assertTrue(full_only['interaction'])
        unknown = b.localize([1, 2, 3], lambda s: {'tasks': list(s), 'outcome': 'unknown'})
        self.assertFalse(unknown['interaction'])
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

    def test_gate_cannot_pass_after_changing_frozen_inputs(self):
        def runner(repo, subset):
            b.git(repo, 'rev-parse', 'HEAD^')
            (repo / 'anchor').write_text('gate changed tracked input\n')
            return 0, b'SYNTHETIC false success\n'
        result = b.execute(self.repo, self.manifest(), self.root / 'receipts', runner)
        self.assertEqual(result['outcome'], 'unknown')
        self.assertIsNone(result['diagnostics'])
        self.assertEqual(result['original_batch']['tasks'], [1, 2, 3])

    def test_disappearing_regression_assertions_cannot_clear_a_member(self):
        m = self.manifest()
        # Replace the last member with an independent regression-check addition.
        b.git(self.repo, 'reset', '--hard', '--quiet', self.fixture['members'][1]['result_sha'])
        parent = self.head()
        (self.repo / 'tests').mkdir()
        (self.repo / 'tests' / 'regression.test.cjs').write_text('throw Error("invented failing regression");\n')
        b.git(self.repo, 'add', '.')
        b.git(self.repo, 'commit', '-qm', 'Synthetic regression member')
        head = self.head()
        item = m['members'][2]
        item['result_sha'] = head
        item['source_export']['result'] = head
        e = item['source_export']['source_readiness']['evidence']
        e['result_sha'] = head
        item['source_export']['source_readiness'].update(current_result_sha=head, result_sha=head)
        for pub in (e['frozen'], e['landed']):
            pub.update(base=parent, parent=parent, head=head, merged=head, integration=head, ci_sha=head)
        item['source_export_sha256'] = b.digest(b.encoded(item['source_export']))
        item['delta_sha256'] = b.digest(b.delta(self.repo, parent, head))
        item['ci']['head_sha'] = item['ci']['observation']['head_sha'] = head
        m['frozen_head'] = head
        def runner(repo, subset):
            return (1 if (repo / 'tests' / 'regression.test.cjs').exists() else 0), b'SYNTHETIC disappearing check\n'
        result = b.execute(self.repo, m, self.root / 'receipts', runner)
        self.assertEqual(result['outcome'], 'failed')
        subset = next(r for r in result['diagnostics']['subsets'] if r['tasks'] == [1, 2])
        self.assertEqual(subset['observed_outcome'], 'passed')
        self.assertEqual(subset['outcome'], 'unknown')
        self.assertIn('check inputs changed', subset['comparability'])
        self.assertIn([1, 2], result['diagnostics']['unknown'])


if __name__ == '__main__':
    unittest.main()
