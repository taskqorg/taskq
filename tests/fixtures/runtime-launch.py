"""Invented native clients for the TaskQ/AgentQ launch boundary; never calls a model."""
import json
import os
from pathlib import Path
import sys
import time
import subprocess

rt = os.environ['TASKQ_FIXTURE_RT']
record = Path(os.environ['TASKQ_FIXTURE_RECORD'])
def save(input_text=None, messages=None, wire=None):
    record.write_text(json.dumps({'args': sys.argv[1:], 'input': input_text,
        'messages': messages, 'wire': wire,
        'env': {key: os.environ.get(key) for key in ('TASKQ_TASK', 'TASKQ_ATTEMPT', 'TASKQ_ROLE',
            'TASKQ_ROOT', 'CARGO_BUILD_JOBS', 'AGENTQ_OPTIMIZATION', 'AGENTQ_LAUNCHER', 'INVENTED_BOARD_SECRET')}}), encoding='utf-8')

if '--version' in sys.argv:
    print('1.5.2' if rt == 'antigravity' else ('2.1.5' if rt == 'claude' else '0.162.0'))
    sys.exit(0)

# One invented background job for owned-stop tests; it never launches a native client or model.
if os.environ.get('TASKQ_FIXTURE_BACKGROUND'):
    state_path = Path(os.environ['TASKQ_FIXTURE_BACKGROUND'])
    state = json.loads(state_path.read_text(encoding='utf-8'))
    audit = state_path.with_suffix('.calls')
    def persist_background():
        state_path.write_text(json.dumps(state), encoding='utf-8')
        home = Path(os.environ['CLAUDE_CONFIG_DIR'])
        locator = home / 'jobs' / state['row']['id'] / 'state.json'
        locator.parent.mkdir(parents=True, exist_ok=True)
        locator.write_text(json.dumps(state['row']), encoding='utf-8')
    with audit.open('a', encoding='utf-8') as log:
        log.write(json.dumps(sys.argv[1:]) + '\n')
    if 'agents' in sys.argv:
        print(json.dumps([state['row']]), flush=True)
    elif 'stop' in sys.argv:
        import signal
        assert sys.argv[-1] == state['row']['id']
        os.kill(state['row']['pid'], signal.SIGTERM)
        state['row'].pop('pid')
        state['row']['state'] = 'stopped'
        state['row']['detail'] = 'invented fixture original process stopped'
        state['row']['status'] = 'idle'
        persist_background()
        print('invented stop acknowledgement', flush=True)
    else:
        assert '--bg' in sys.argv
        literal = sys.stdin.read()
        session = (sys.argv[sys.argv.index('--session-id') + 1]
            if '--session-id' in sys.argv else state['row']['sessionId'])
        state['row']['sessionId'] = session
        state['row']['intent'] = literal
        home = Path(os.environ['CLAUDE_CONFIG_DIR'])
        key = ''.join(c if c.isascii() and c.isalnum() else '-' for c in state['row']['cwd'])
        source = home / 'projects' / key / (session + '.jsonl')
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_text(json.dumps({'type': 'system', 'subtype': 'init', 'session_id': session}) + '\n', encoding='utf-8')
        persist_background()
        save(literal)
        print('backgrounded · ' + state['row']['id'], flush=True)
    sys.exit(0)

if 'acp' in sys.argv:
    messages = []
    session = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'
    for line in sys.stdin:
        message = json.loads(line)
        method = message.get('method')
        messages.append(method)
        save(messages=messages)
        result = {}
        if method == 'initialize':
            result = {'protocolVersion': 1, 'agentCapabilities': {'loadSession': True},
                'agentInfo': {'name': 'invented-fixture', 'version': '1.5.2'}}
        elif method in ('session/new', 'session/load'):
            result = {'sessionId': ('foreign-session' if os.environ.get('TASKQ_FIXTURE_WRONG_SESSION') else session)}
        elif method == 'session/prompt':
            literal = '\n'.join(part.get('text', '') for part in message['params']['prompt'])
            save(literal, messages, message)
            print(json.dumps({'jsonrpc': '2.0', 'method': 'session/update', 'params': {
                'sessionId': session, 'update': {'sessionUpdate': 'agent_message_chunk',
                    'content': {'type': 'text', 'text': 'fixture done'}}}}), flush=True)
            result = {'stopReason': 'end_turn'}
        if 'id' in message:
            print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
        if method == 'session/prompt' and os.environ.get('TASKQ_FIXTURE_LINGER'):
            time.sleep(60)
    sys.exit(0)

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
    if rt == 'claude':
        for flag in ('--session-id', '--resume'):
            if flag in sys.argv:
                session = sys.argv[sys.argv.index(flag) + 1]
    rows = {
        'codex': [{'type': 'thread.started', 'thread_id': session},
            {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'fixture done'}},
            {'type': 'turn.completed'}],
        'claude': [{'type': 'system', 'subtype': 'init', 'session_id': session},
            {'type': 'assistant', 'message': {'id': 'invented-call', 'model': 'invented-model',
                'usage': {'input_tokens': 2, 'cache_read_input_tokens': 3, 'cache_creation_input_tokens': 0, 'output_tokens': 4}}},
            {'type': 'result', 'subtype': 'success', 'result': 'fixture done', 'is_error': False}],
        'antigravity': [{'event': 'init', 'conversation_id': session},
            {'event': 'result', 'result': {'conversation_id': session, 'status': 'SUCCESS', 'response': 'fixture done'}}],
    }
    for row in rows[rt]:
        print(json.dumps(row), flush=True)
    if os.environ.get('TASKQ_FIXTURE_LINGER'):
        time.sleep(60)
