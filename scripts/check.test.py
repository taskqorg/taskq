"""R12: doc-only selection cannot hide Rust/config/unknown or missing impact."""
from pathlib import Path
import runpy

check = runpy.run_path(str(Path(__file__).with_name('check.py')))
select, names = check['selection'], check['runtime_names']
for paths, expected in [([], 'full'), (['README.md'], 'docs'), (['AGENTS.md'], 'full'),
                        (['taskq.md'], 'full'), (['docs/design/answer-card.md'], 'web'),
                        (['src/tray.html', 'tests/tray-action.test.cjs'], 'web'),
                        (['src/core.rs', 'README.md'], 'full'), (['Cargo.lock'], 'full'),
                        (['tests/fixtures/status-snapshot.json'], 'full'),
                        (['.github/workflows/tests.yml'], 'full'), (['unknown'], 'full')]:
    assert select(paths) == expected, (paths, select(paths), expected)
print('PASS change selection; unknown impact broadens')

# Owner-operation fixtures must not inherit a live worker identity or board credential; the launching session stays bound.
import json
import os
import subprocess
import sys
import tempfile
with tempfile.TemporaryDirectory(prefix='taskq-check-context-') as install:
    parent = {**os.environ, 'TASKQ_TASK': '1840', 'TASKQ_ATTEMPT': 'invented-attempt', 'TASKQ_ROLE': 'worker',
              'TASKQ_ROOT': 'invented-project', 'TASKQ_INSTALL': 'invented-production-install', 'CODEX_THREAD_ID': 'invented-thread', 'GH_TOKEN': 'invented-token', 'CARGO_BUILD_JOBS': '2', 'TASKQ_INCIDENT_REPORTS': 'off'}
    child = check['test_environment'](check['check_environment'](parent), install)
    observed = json.loads(subprocess.check_output([sys.executable, '-c',
        'import os,json;print(json.dumps({k:os.environ.get(k) for k in ["TASKQ_TASK","TASKQ_ATTEMPT","TASKQ_ROLE","TASKQ_ROOT","CODEX_THREAD_ID","GH_TOKEN","TASKQ_INSTALL","CARGO_BUILD_JOBS","TASKQ_INCIDENT_REPORTS"]}))'], env=child))
    assert observed == {'TASKQ_TASK': None, 'TASKQ_ATTEMPT': None, 'TASKQ_ROLE': None, 'TASKQ_ROOT': None, 'CODEX_THREAD_ID': None,
                        'GH_TOKEN': None, 'TASKQ_INSTALL': install, 'CARGO_BUILD_JOBS': '2', 'TASKQ_INCIDENT_REPORTS': None}
    assert parent['TASKQ_TASK'] == '1840' and parent['GH_TOKEN'] == 'invented-token' and parent['TASKQ_INSTALL'] == 'invented-production-install'
print('PASS isolated fixture installation, worker identity and credentials; launching context preserved')

# #1940: reduce process oversubscription, retaining all fixture deadlines and explicit CI overrides.
env = {k:v for k,v in os.environ.items() if k != 'RUST_TEST_THREADS'}
shaped = check['test_environment'](env, 'invented-install')
if sys.platform == 'win32':
    assert shaped['RUST_TEST_THREADS'] == str(min(4, os.cpu_count() or 1))
assert check['test_environment']({**env, 'RUST_TEST_THREADS':'2'}, 'invented-install')['RUST_TEST_THREADS'] == '2'
print('PASS bounded Windows harness concurrency; explicit override preserved')

assert names('src/tray.rs', 'if rt == "claude" {}') == ['src/tray.rs:1: if rt == "claude" {}']
assert names('src/tray.rs', 'of(rt).remote\n#[cfg(test)]\nmod tests {\nlet rt = "codex";\n}') == []
# Code after a test-only item or test module is still scanned.
assert names('src/x.rs', '#[cfg(test)]\nfn f() {}\nlet a = "codex";') == ['src/x.rs:3: let a = "codex";']
assert names('src/x.rs', '#[cfg(test)]\nmod t {\n    fn f() { "claude"; }\n}\nlet a = "codex";') == ['src/x.rs:5: let a = "codex";']
assert names('src/x.rs', '#[cfg(test)]\npub(crate) mod tests {\n"codex";\n}') == []
# Name-prefixed literals count.
assert names('src/x.rs', 's.starts_with("claude:")') == ['src/x.rs:1: s.starts_with("claude:")']
assert names('src/runtime.rs', 'rt == "codex-app"') == names('src/runtime/owner.rs', '".claude.json"') == []
assert names('src/windows_sandbox.rs', '"codex"') == names('src/windows_sandbox/acl.rs', '"claude"') == names('src/linux_sandbox.rs', 'CODEX_HOME') == []
# #1593: a planted bypass in main.rs or a former exception fails, native names included
for planted in ['if k.runtime=="claude" {}', 'tasklist claude.exe', 'home.join(".claude.json")', 'env("CODEX_HOME", h)', 'env_remove("CODEX_THREAD_ID")']:
    for path in ['src/main.rs', 'src/labels.rs', 'src/host.rs', 'src/setup.rs']:
        assert names(path, planted) == [f'{path}:1: {planted}'], (path, planted)
print('PASS runtime names stay in the adapter')

# Rule-change gate (docs/design/rule-ids.md#contract-detail). Defect: bf4a2be9 (#1776) rewrote the M3 host strip
# rule and its tests, ecb1e57d dropped `Take requests`, both unnamed by their tasks; review missed both.
# Risk kept: a rule or rule-citing test changes or disappears unless the task names its ID.
blocks, findings, unnamed, uncovered = check['rule_blocks'], check['rule_findings'], check['unnamed'], check['uncovered']
root = Path(__file__).resolve().parents[1]
head = {p: (root / p).read_text(encoding='utf-8') for p in check['RULE_DOCS']}
ids = check['doc_rules'](head)
assert {'R1', 'R13', '§1', '§9', '§2 Ask card', 'Change rule', '§2.1', '§2.25', 'M3', 'M3.25'} <= ids.keys(), sorted(ids)
assert 'host strip' in ids['M3.1'] and '`hold`' in ids['§2.2'] and '`hold`' not in ids['§2'], ids['M3.1']
spec = '# t\npre\n## Principles (R1–R2)\n### R1. One\nalpha beta gamma.\n### R2. Two\nx\n## 2. Log\nlog text\n| a | b | ID |\n|---|---|---|\n| x | `y` | 1 |\n### Ask card (#9)\ncard text\n[l]: http://x\n'
doc = lambda text: {'taskq.md': text, 'docs/design/menu.md': ''}
assert blocks('taskq.md', spec) == {'Principles': '## Principles (R1–R2)', 'R1': '### R1. One alpha beta gamma.', 'R2': '### R2. Two x',
                                    '§2': '## 2. Log log text | a | b | ID | |---|---|---|', '§2.1': '| x | `y` | 1 |', '§2 Ask card': '### Ask card (#9) card text'}
assert blocks('docs/design/menu.md', '## M3 Nav\nintro\n- M3.2 two\n\nM3.1 one\nmore\n\nafter\n## Acceptance\nx\n') == \
    {'M3': '## M3 Nav intro after', 'M3.2': '- M3.2 two', 'M3.1': 'M3.1 one more'}
none = {'changed': [], 'removed': [], 'tests': []}
# A typo fix passes unnamed; a meaning word, a number, code or a third word does not.
assert findings(doc(spec), doc(spec.replace('gamma.', 'gama.')), '') == none
for edit in [('beta', 'not'), ('pre', 'changed preamble'), ('x |', 'z |'), ('beta gamma', 'beta gamma delta'), ('Two\nx', 'Two\n2')]:
    expected = {'beta': ['R1'], 'pre': [], 'x |': ['§2.1'], 'beta gamma': ['R1'], 'Two\nx': ['R2']}[edit[0]]
    assert findings(doc(spec), doc(spec.replace(*edit, 1)), '')['changed'] == expected, edit
assert findings(doc(spec), doc(spec.replace('alpha beta', 'alpha deta').replace('gamma', 'gama').replace('One', 'Onn')), '')['changed'] == ['R1']
# Removals need a destination; `§2 Ask card` does not name `§2`, `§ 2` does not name `§2.1`.
gone = findings(doc(spec), doc(spec.replace('### R2. Two\nx\n', '')), '')
assert gone == {'changed': [], 'removed': ['R2'], 'tests': []}, gone
assert unnamed(gone, 'drops R2') == ['R2 (removed; name `R2 -> <ID, file or none>`)'] and unnamed(gone, 'R2 -> R1') == []
both = {'changed': ['§2', '§2.1', '§2 Ask card'], 'removed': [], 'tests': []}
assert unnamed(both, 'edits § 2 Ask card and §2.1') == ['§2 (changed)'] and unnamed(both, '§ 2, §2.1 and §2 Ask card') == []
# A changed or removed assertion or Chain: line citing an ID is listed; a moved one and an uncited one are not.
diff = '-    // Chain: § 2 x\n+    // Chain: § 2 y\n-assert!(a, "R1 b");\n+assert!(a, "R1 b");\n-assert!(c);\n-let s = "R2";\n'
assert findings(doc(spec), doc(spec), diff)['tests'] == ['§2'], findings(doc(spec), doc(spec), diff)
assert uncovered({'R1': '', '§2': '', '§2.1': ''}, '// Chain: § 2 table, R1') == ['§2.1']
print('PASS rule IDs, typo fixes and the gate on synthetic contracts')

# #1826 R13: the owner named "section 7" and its exact subsection. Preserve ID boundaries and require both;
# accepting the ordinary spelling must not approve another section, table row or unnamed change.
report_rules = {'changed': ['§7', '§7 Result and close verdicts'], 'removed': [], 'tests': ['§7']}
assert unnamed(report_rules, 'amend section 7 and section 7 Result and close verdicts') == []
assert unnamed(report_rules, 'amend section 7 Result and close verdicts') == ['§7 (changed)', '§7 (tests)']
assert unnamed(report_rules, 'amend section 70') == ['§7 (changed)', '§7 Result and close verdicts (changed)', '§7 (tests)']

# Replays against their parents; the CI plan job checks out full history.
for rev, listed in [('bf4a2be9', {'changed': ['M3'], 'tests': ['R12']}), ('ecb1e57d', {'removed': ['§7 Take requests'], 'changed': ['R1', '§7']})]:
    found = findings(check['git_docs'](rev + '^'), check['git_docs'](rev), check['test_diff'](rev + '^', rev))
    assert all(set(v) <= set(found[k]) for k, v in listed.items()), (rev, found)
    assert unnamed(found, 'Fix a typo.'), rev
    task = ' '.join(found['changed'] + found['tests'] + [f'{k} -> none' for k in found['removed']])
    assert unnamed(found, task) == [], (rev, unnamed(found, task))
# #2118: preserve plural section citations without approving unnamed sections, rows or subsections.
plural = {'changed': ['§4', '§9', '§3', '§4.1', '§4 Profiling'], 'removed': [], 'tests': []}
assert unnamed(plural, 'sections 4 Filter and 9') == ['§3 (changed)', '§4.1 (changed)', '§4 Profiling (changed)']
assert unnamed(plural, 'sections 4 and 90') == ['§9 (changed)', '§3 (changed)', '§4.1 (changed)', '§4 Profiling (changed)']
assert unnamed(plural, 'sections 4 Profiling and 9') == ['§4 (changed)', '§3 (changed)', '§4.1 (changed)']
print('PASS replays of bf4a2be9 and ecb1e57d fail unnamed and pass named')


# #1818 / #1805: the owner picked the design, but Product appeared only in the ask's context.
# Keep the real task_text -> approval gate boundary; the subprocesses return invented board data.
import json
from types import SimpleNamespace
subprocess = check['task_text'].__globals__['subprocess']
old_run, old_output = subprocess.run, subprocess.check_output
ask = {'id': 10, 'body': 'taskq: ask attempt 1\nProblem: Approval updates Product and M2.\n1. A; native at release - + compact; - later qualification\n2. B - + wider; - changes R9',
       'author_association': 'OWNER', 'created_at': 'invented', 'updated_at': 'invented'}
answer = {**ask, 'id': 11, 'body': 'taskq: answer 1\nA; native at release - + compact; - later qualification'}
try:
    subprocess.check_output = lambda *a, **k: b'Task: https://github.com/invented/project/issues/1\n'
    def board_read(command, **kwargs):
        if command[-1].endswith('/comments'):
            out = json.dumps([[{**ask, 'id': 1, 'body': 'taskq: spawn worker codex:invented win'}, ask, answer]]) if '--slurp' in command else answer['body']
        else:
            out = 'Design A is approved by the owner.'
        return SimpleNamespace(returncode=0, stdout=out.encode(), stderr=b'')
    subprocess.run = board_read
    approved, error = check['task_text']('invented-base')
    assert error is None, error
    assert unnamed({'changed': ['Product', 'M2'], 'removed': [], 'tests': []}, approved) == [], approved
    assert 'R9' not in approved, 'the unchosen option is not approval'
finally:
    subprocess.run, subprocess.check_output = old_run, old_output
print('PASS selected design approval reads its ask without asking the owner again')

# Pair approvals with the accepted question, never stale revisions, ignored asks or other options.
approval = lambda comments: check['approval_text'](comments, 99)
comment = lambda i, body, **fields: {**ask, 'id': i, 'body': body, **fields}
spawn = comment(1, 'taskq: spawn worker codex:invented win')
for answer_body in ['taskq: answer 1', '1', '1. A; native at release - + compact; - later qualification',
                    'taskq: answer\nPick 1. Proceed.', 'taskq answer 1', 'taskq answer 99.1', 'taskq answer #99.1']:
    text = approval([spawn, ask, comment(11, answer_body)])
    assert 'Product' in text and 'M2' in text and 'R9' not in text, (answer_body, text)
for rejected in [comment(11, 'taskq: answer 1\nR12\nop-id: stale\nrevision: 9'),
                 comment(11, 'taskq: answer 1 attempt 1'),
                 comment(11, '1', updated_at='edited'),
                 comment(11, '1', author_association='NONE')]:
    assert approval([spawn, ask, rejected]) == '', rejected
new_ask = comment(13, 'taskq: ask attempt 1\nProblem: New M3 choice\n1. keep\n2. change')
text = approval([spawn, ask, comment(11, 'taskq answer 2'), new_ask, comment(14, '1')])
assert 'M3' in text, text
replacement = [spawn, comment(2, 'taskq: requeue'), comment(3, 'taskq: spawn worker codex:new win'),
               comment(4, 'taskq: ask attempt 1\nProblem: Ignored R9\n1. old\n2. other'),
               comment(5, 'taskq: ask attempt 3\nProblem: Current M2\n1. keep\n2. change'), comment(6, '1')]
text = approval(replacement)
assert 'M2' in text and 'R9' not in text, text
for prefix in [
    [spawn, comment(2, 'taskq: spawn worker codex:ignored win')],
    [spawn, comment(2, 'taskq: result invalid attempt 1')],
    [comment(1, 'taskq: hold'), comment(2, 'taskq: requeue')],
    [spawn, comment(2, 'taskq: result ' + 'a'*40 + ' attempt 1'), comment(3, 'taskq: close'),
     comment(4, 'taskq: later'), comment(5, 'taskq: requeue')],
    [spawn, comment(2, 'taskq: requeue attempt 1')],
]:
    invalid = prefix + [comment(7, 'taskq: spawn reviewer codex:ignored win'),
                        comment(8, 'taskq: ask attempt 7\nProblem: Ignored R9\n1. yes\n2. no'), comment(9, '1')]
    assert 'R9' not in approval(invalid), invalid
replacement[1] = comment(2, 'taskq: requeue\nop-id: reset\nrevision: 1')
assert 'M2' in approval(replacement), 'a fenced reset must consume the old attempt'
assert approval([spawn, comment(2, 'taskq: requeue attempt 1'),
                 comment(3, 'taskq: ask attempt 1\nProblem: Ignored R9\n1. yes\n2. no'), comment(4, '1')]) == ''
assert approval([spawn, comment(2, 'taskq:  requeue\tattempt 1'),
                 comment(3, 'taskq: ask attempt 1\nProblem: Ignored R9\n1. yes\n2. no'), comment(4, '1')]) == ''
assert 'R9' not in approval([spawn, comment(2, 'taskq: ask attempt 1\nProblem: Choose\n1. keep\n1. change R9\n2. other'),
                            comment(3, 'taskq: answer 1')]), 'duplicate numbers retain the first option, as fold does'
assert 'R9' not in approval([spawn, comment(2, 'taskq: ask attempt 1\nProblem: Choose\n  1. keep\n  2. change R9'),
                            comment(3, 'taskq: answer 1')]), 'indented unchosen options are not ask context'
assert 'R9' not in approval([spawn, comment(2, 'taskq: ask attempt 1\nProblem: First\n1. keep\n2. other'),
                            comment(3, 'taskq: answer 1\nop-id: repeated id'),
                            comment(4, 'taskq: ask attempt 1\nProblem: New\n1. change R9\n2. keep'),
                            comment(5, 'taskq: answer 1\nop-id: repeated id')]), 'duplicate operations cannot approve a new ask'
# #1918: a reviewer requeue readied the task, but the gate dropped the next attempt's approved M8 ask.
def review_requeue(i, flag='', worker=None):
    return [comment(i, 'taskq: result ' + 'a'*40 + f' attempt {worker or i - 1}'), comment(i + 1, 'taskq: spawn reviewer claude:invented win'),
            comment(i + 2, f'taskq: requeue{flag} attempt {i + 1}'), comment(i + 3, 'taskq: spawn worker claude:invented win')]
approve_m8 = lambda i, rule='M8': [comment(i, f'taskq: ask attempt {i - 1}\nProblem: Approve {rule}\n1. approve {rule}\n2. keep R9'), comment(i + 1, '1')]
text = approval([spawn, *review_requeue(2), *approve_m8(6)])
assert 'M8' in text and 'R9' not in text, text
three = [spawn, *review_requeue(2), *review_requeue(6), *review_requeue(10)]
assert approval(three + approve_m8(14)) == '', 'the 3rd retry asks with the limit card, which fold models outside this gate'
three[7] = comment(8, 'taskq: requeue main-moved attempt 7')
assert 'M8' in approval(three + approve_m8(14)), 'main-moved keeps retries'
assert 'M8' in approval([spawn, *review_requeue(2), *approve_m8(6, 'M3'), *review_requeue(8, worker=5), *review_requeue(12), *approve_m8(16)]), \
    'an answer resets retries'
print('PASS approval association preserves revision, attempt, trust and selected-option fences')
