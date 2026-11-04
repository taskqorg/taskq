"""Release contract: stale, foreign and wrong-workflow success cannot publish."""
from pathlib import Path
import os
import re
import runpy
import subprocess

select = runpy.run_path(str(Path(__file__).with_name('release-check.py')))['selected']
base = dict(id=1, head_branch='main', head_sha='a'*40, path='.github/workflows/tests.yml', event='workflow_dispatch', created_at='2026-10-17T00:00:00Z', status='completed', conclusion='success')
now = 1792198800  # 2026-10-17T01:00:00Z
assert select([base], 'a'*40, 'main', now) == base
for edit in [dict(head_sha='b'*40), dict(head_branch='another-branch'), dict(path='.github/workflows/other.yml'), dict(event='workflow_run'), dict(event='pull_request'), dict(event='push'), dict(created_at='bad'), dict(created_at='2026-09-01T00:00:00Z'), dict(created_at='2026-10-18T00:00:00Z')]:
    assert select([{**base, **edit}], 'a'*40, 'main', now) is None, edit
failed = {**base, 'id':2, 'conclusion':'failure'}
assert select([base, failed], 'a'*40, 'main', now) == failed, 'newest failure cannot reuse earlier success'
pending = {**base, 'id':3, 'status':'in_progress', 'conclusion':None}
assert select([base, failed, pending], 'a'*40, 'main', now) == pending
assert select([{**base, 'head_branch':'v9'}], 'a'*40, 'main', now) is None
queue = {**base, 'id':4, 'event':'merge_group', 'head_branch':'gh-readonly-queue/main/pr-1-'+'c'*40}
assert select([queue], 'a'*40, 'main', now) == queue
assert select([{**queue, 'head_branch':'gh-readonly-queue/other/pr-1'}], 'a'*40, 'main', now) is None
push_pending = {**pending, 'id':5}
assert select([queue, push_pending], 'a'*40, 'main', now) == queue, 'queue success need not wait for a pending dispatch run'
assert select([{**queue, 'conclusion':'failure'}, push_pending], 'a'*40, 'main', now) == push_pending
assert select([queue, {**failed, 'id':5}], 'a'*40, 'main', now)['conclusion'] == 'failure', 'newer dispatch failure still refuses'
assert select([{**base, 'created_at':'2026-10-01T00:00:00Z'}], 'a'*40, 'main', now), 'a release may promote a commit tested weeks ago'
print('PASS exact-source release check, freshness, latest failure/pending isolation and merge-queue exact source')

# Chain: §8. Only an explicit main demand or a live/tag push reaches the guard before tags/builds; ui-live publishes the interface alone.
workflow = Path(__file__).with_name('workflows').joinpath('release.yml').read_text(encoding='utf-8')
job = lambda name: re.search(r'(?ms)^  '+name+r':\n(.*?)(?=^  [a-z][a-z-]*:|\Z)', workflow[workflow.index('\njobs:\n'):])[1]
names = re.findall(r'(?m)^  ([a-z][a-z-]*):$', workflow[workflow.index('\njobs:\n'):])
assert re.search(r'(?m)^  workflow_dispatch:$', workflow)
assert re.search(r"(?m)^    tags: \['v\*'\]$", workflow), 'manual tag pushes stay supported'
assert re.search(r'(?m)^    branches: \[live, ui-live\]$', workflow), 'main pushes must not request releases'
assert not re.search(r'(?m)^  schedule:', workflow), 'there is no release timer'
assert re.search(r'(?m)^    needs: checks$', job('tag')), 'tag creation must wait for exact-source CI'
assert not re.search(r'(?m)^    needs:', job('checks')), 'the source guard must run before tag output exists'
assert "if: github.event_name == 'workflow_dispatch'" in job('checks')
assert 'cancel-in-progress: false' in workflow, 'an active publisher must finish'
assert re.search(r'(?m)^  queue: max$', workflow), 'an old rerun must not replace a newer pending release'
for name in ['build-linux','build-windows','build-macos']:
    assert re.search(r'(?m)^    needs: tag$', job(name)), name
for name in ['publish-linux','publish-windows','publish-macos']:
    assert re.search(r'(?m)^    needs: \[tag, checks, build-%s\]$' % name[len('publish-'):], job(name)), name
assert 'release-check.py' in job('checks') and 'release-check.py' in job('publish-ui')
print('PASS demand-only main trigger keeps checks before tags/builds and serializes publication')

# The real demand guard runs on each ref; a refusal must stop everything before publication.
guard = re.search(r'(?m)^          (\[ "\$REQUEST_REF".*)$', job('checks'))[1]
def refused(ref):
    return subprocess.run(['sh', '-c', guard], env={**os.environ, 'REQUEST_REF': ref}, capture_output=True).returncode != 0
assert not refused('refs/heads/main')
WRONG_REFS = ['refs/heads/ui-live', 'refs/heads/live', 'refs/heads/feature', 'refs/heads/main2', 'refs/tags/v9.9.9']
for ref in WRONG_REFS:
    assert refused(ref), ref

def condition(text, event, ref, tag_output):
    """Evaluate one job condition; an unknown expression fails instead of reading as true."""
    context = {'github.event_name': event, 'github.ref_name': ref.rsplit('/', 1)[1], 'github.ref': ref, 'needs.tag.outputs.tag': tag_output}
    python = text.replace('&&', ' and ').replace('||', ' or ')
    for name, value in context.items():
        python = python.replace(name, repr(value))
    assert re.fullmatch(r"(?:'[^']*'|==|!=|and|or|\s)+", python), text
    return bool(eval(python))

def simulate(event, ref, checks_pass=True, tag_output='v1.2.3'):
    """GitHub job semantics: without a status function, a job runs only when each need succeeded."""
    ran = {}
    pending = list(names)
    while pending:
        name = pending.pop(0)
        need = re.search(r'(?m)^    needs: (.+)$', job(name))
        needs = re.findall(r'[a-z][a-z-]*', need[1]) if need else []
        if any(n not in ran for n in needs):
            pending.append(name)
            continue
        when = re.search(r'(?m)^    if: (.+)$', job(name))
        ran[name] = all(ran[n] for n in needs) and (not when or condition(when[1], event, ref, tag_output))
        if name == 'checks' and ran[name]:
            ran[name] = checks_pass and not (event == 'workflow_dispatch' and refused(ref))
    return ran

BINARY = {'tag', 'build-linux', 'build-windows', 'build-macos', 'publish-linux', 'publish-windows', 'publish-macos', 'seal'}
PUBLISHING = BINARY | {'publish-ui'}
assert PUBLISHING | {'checks'} == set(names), names
for event, ref in [('workflow_dispatch', 'refs/heads/main'), ('push', 'refs/heads/live'), ('push', 'refs/tags/v9.9.9')]:
    ran = simulate(event, ref)
    assert ran['checks'] and not ran['publish-ui'] and all(ran[n] for n in BINARY), (event, ref, ran)
    ran = simulate(event, ref, checks_pass=False)
    assert not any(ran[n] for n in PUBLISHING), ('failed exact-source CI publishes nothing', event, ref, ran)
ran = simulate('workflow_dispatch', 'refs/heads/main', tag_output='')
assert ran['tag'] and not any(ran[n] for n in PUBLISHING - {'tag'}), ('an already released demand builds nothing', ran)
for ref in WRONG_REFS:
    ran = simulate('workflow_dispatch', ref)
    assert not any(ran[n] for n in PUBLISHING), ('wrong dispatch ref publishes nothing', ref, ran)
ran = simulate('push', 'refs/heads/ui-live')
assert ran['publish-ui'] and not ran['checks'] and not any(ran[n] for n in BINARY), ('a ui-live push publishes the interface alone', ran)
print('PASS main dispatch, wrong refs including ui-live, ordering, UI-only push and live/tag paths')

# Permission and credential boundary: each job's grant is exact; the only credential is the run token.
PUBLISH = {'contents': 'write', 'id-token': 'write', 'attestations': 'write'}
GRANTS = {'publish-ui': {**PUBLISH, 'actions': 'read'}, 'tag': {'contents': 'write'}, 'checks': {'contents': 'read', 'actions': 'read'},
          'publish-linux': PUBLISH, 'publish-windows': PUBLISH, 'publish-macos': PUBLISH,
          'seal': {'contents': 'read', 'actions': 'write', 'id-token': 'write', 'attestations': 'write'}}
def grant(name):
    block = re.search(r'(?m)^    permissions:\n((?:      [a-z-]+: [a-z]+\n)+)', job(name))
    return dict(re.findall(r'([a-z-]+): ([a-z]+)', block[1])) if block else None
assert {n: grant(n) for n in GRANTS} == GRANTS
assert all(grant(n) is None for n in names if n.startswith('build-')), 'reusable builds keep their own permissions'
assert re.search(r'(?m)^permissions:\n  contents: read$', workflow), 'the workflow default stays read-only'
assert 'secrets' not in workflow, 'release adds no credential beyond github.token'
print('PASS permission and credential boundary is unchanged')
