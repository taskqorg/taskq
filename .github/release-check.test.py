"""Release contract: stale, foreign and wrong-workflow success cannot publish."""
from pathlib import Path
import runpy

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
