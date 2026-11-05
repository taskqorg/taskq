#!/usr/bin/env python3
"""R12/#2337: every lifecycle event still needs valid executor identity to prove a trace."""
import json
from pathlib import Path
import subprocess
import sys

# Execute the actual trace validator with exported observations; no native host or board is needed.
script = Path(__file__).with_name('telemetry.sh').read_text()
validator = script.split('  "$PY" -I -c \'\n', 1)[1].split("\n' \"$task\"", 1)[0]
events = ['spawn', 'heartbeat', 'result', 'exit', 'slot-hold', 'slot-release']
rows = [dict(kind='worker', task=2337, attempt=42, project='invented/repo', host='invented-host',
             runtime='codex', source='work', process_id=12345, operation=e, to='fixture', started_ms=1000)
        for e in events]
rows.append(dict(kind='transition', task=2337, project='invented/repo', to='closed', started_ms=2000))
header = dict(sources=dict(universal=dict(samples=7)))

def run(expected):
    exported = '\n'.join(json.dumps(x) for x in [header] + [dict(observation=o) for o in rows]) + '\n'
    result = subprocess.run([sys.executable, '-I', '-c', validator, '2337', '', '24h'],
                            input=exported, capture_output=True, text=True)
    assert result.returncode == expected, result.stdout + result.stderr
    assert ('verdict: PASS' in result.stdout) == (expected == 0), result.stdout

run(0)
for field, value in [('process_id', 0), ('process_id', None), ('process_id', 1), ('process_id', '12345'),
                     ('attempt', 0), ('attempt', None), ('host', ''), ('runtime', 'unknown'), ('source', None)]:
    old = rows[5][field]
    rows[5][field] = value
    run(1)
    rows[5][field] = old
del rows[5]['process_id']
run(1)
print('PASS trace accepts complete identity and refuses zero, missing or unknown lifecycle identity')
