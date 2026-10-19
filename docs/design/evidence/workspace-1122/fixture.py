"""Invented native workspace evidence, on top of docs/shots/fixture.sh; no service runs."""
import json
import pathlib
import sys
base = pathlib.Path(sys.argv[1]).resolve()
requests = base / 'storefront/.taskq/requests'
requests.mkdir(exist_ok=True)
(requests / '100.json').write_text(json.dumps({'text': 'Review the complete exchange before confirming these changes.'}))
(requests / '100.answer.json').write_text(json.dumps({
    'state': 'proposal', 'rev': 2,
    'text': 'The full reply remains attached to this conversation.\nReview every target and effect below.\nNo changes have been applied.',
    'ops': [{'op': 'later', 'project': 'storefront', 'task': 44, 'text': 'Owner requested a pause', 'revision': 41}],
    'effects': ['storefront #44: ready → later; stop future admission for this task.'],
}))
(base / 'install/tray-requests.json').write_text(json.dumps({
    'selected': {'root': str(base / 'storefront'), 'id': '100'},
    'history': ['Review the complete exchange before confirming these changes.'], 'draft': '',
}))
