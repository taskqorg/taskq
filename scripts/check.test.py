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
assert names('src/runtime.rs', 'rt == "codex-app"') == names('src/labels.rs', '"claude"') == []
print('PASS runtime names stay in the adapter')
