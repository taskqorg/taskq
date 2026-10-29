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

# Replays against their parents; the CI plan job checks out full history.
for rev, listed in [('bf4a2be9', {'changed': ['M3'], 'tests': ['R12']}), ('ecb1e57d', {'removed': ['§7 Take requests'], 'changed': ['R1', '§7']})]:
    found = findings(check['git_docs'](rev + '^'), check['git_docs'](rev), check['test_diff'](rev + '^', rev))
    assert all(set(v) <= set(found[k]) for k, v in listed.items()), (rev, found)
    assert unnamed(found, 'Fix a typo.'), rev
    task = ' '.join(found['changed'] + found['tests'] + [f'{k} -> none' for k in found['removed']])
    assert unnamed(found, task) == [], (rev, unnamed(found, task))
print('PASS replays of bf4a2be9 and ecb1e57d fail unnamed and pass named')
