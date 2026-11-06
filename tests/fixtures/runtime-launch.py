"""Invented native clients for the TaskQ/AgentQ launch boundary; never calls a model."""
import json
import os
from pathlib import Path
import sys
import time
import subprocess

rt = os.environ['TASKQ_FIXTURE_RT']
record = Path(os.environ['TASKQ_FIXTURE_RECORD'])
if os.environ.get('TASKQ_FIXTURE_TREE'):
    descendant = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'],
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    Path(os.environ['TASKQ_FIXTURE_TREE']).write_text(str(descendant.pid), encoding='utf-8')
record.write_text(json.dumps({'args': sys.argv[1:], 'input': sys.stdin.read(),
    'env': {key: os.environ.get(key) for key in ('TASKQ_TASK', 'TASKQ_ATTEMPT', 'TASKQ_ROLE',
        'TASKQ_ROOT', 'CARGO_BUILD_JOBS', 'AGENTQ_OPTIMIZATION', 'AGENTQ_LAUNCHER', 'INVENTED_BOARD_SECRET')}}), encoding='utf-8')
if '--bg' in sys.argv:
    print('backgrounded · aaaaaaaa', flush=True)
    print('invented launcher diagnostic', file=sys.stderr, flush=True)
else:
    if os.environ.get('TASKQ_FIXTURE_TREE'):
        print(json.dumps({'type': 'thread.started', 'thread_id': 'invented-scope-thread'}), flush=True)
        while not Path(os.environ['TASKQ_FIXTURE_TREE'] + '.exit').exists():
            time.sleep(0.01)
        sys.exit(0)
    session = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
    rows = {
        'codex': [{'type': 'thread.started', 'thread_id': session},
            {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'fixture done'}},
            {'type': 'turn.completed'}],
        'claude': [{'type': 'system', 'subtype': 'init', 'session_id': session},
            {'type': 'result', 'subtype': 'success', 'result': 'fixture done', 'is_error': False}],
        'antigravity': [{'event': 'init', 'conversation_id': session},
            {'event': 'result', 'result': {'conversation_id': session, 'status': 'SUCCESS', 'response': 'fixture done'}}],
    }
    for row in rows[rt]:
        print(json.dumps(row), flush=True)
    if os.environ.get('TASKQ_FIXTURE_LINGER'):
        time.sleep(60)
