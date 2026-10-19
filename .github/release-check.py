#!/usr/bin/env python3
"""Publication requires recent successful tests.yml for the exact source commit.

The merge queue tests the commit that main fast-forwards to, so its merge_group run on the same SHA counts
while the duplicate push run is still pending; a newer completed failure still refuses."""
import datetime
import json
import os
import subprocess
import time


def selected(runs, sha, branch, now):
    accepted = []
    for run in runs:
        if run.get('head_sha') != sha or run.get('path') != '.github/workflows/tests.yml':
            continue
        if run.get('event') in {'push', 'workflow_dispatch'}:
            if run.get('head_branch') != branch:
                continue
        elif run.get('event') != 'merge_group' or not str(run.get('head_branch')).startswith(f'gh-readonly-queue/{branch}/'):
            continue
        try:
            created = datetime.datetime.fromisoformat(run['created_at'].replace('Z', '+00:00')).timestamp()
        except (KeyError, TypeError, ValueError):
            continue
        if 0 <= now - created <= 86400:
            accepted.append(run)
    newest = max(accepted, key=lambda r: r['id'], default=None)
    if newest and newest.get('status') != 'completed':
        queued = [r for r in accepted if r.get('event') == 'merge_group' and r.get('status') == 'completed']
        if queued and max(queued, key=lambda r: r['id']).get('conclusion') == 'success':
            return max(queued, key=lambda r: r['id'])
    return newest


def main():
    sha = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    repo = os.environ['GITHUB_REPOSITORY']
    until = time.monotonic() + 900
    while True:
        left = until - time.monotonic()
        if left <= 0:
            raise SystemExit('Exact-source tests unavailable within 900s; publication refused')
        payload = subprocess.check_output(['gh', 'api', f'repos/{repo}/actions/workflows/tests.yml/runs?head_sha={sha}&per_page=100'], text=True, timeout=min(30, left))
        run = selected(json.loads(payload)['workflow_runs'], sha, os.environ['GITHUB_REF_NAME'], time.time())
        if run and run['status'] == 'completed':
            if run['conclusion'] != 'success':
                raise SystemExit(f"Exact-source tests failed: {run['html_url']}")
            print(f"Exact-source tests passed: {sha} {run['html_url']}")
            return
        print('Waiting for exact-source tests; publication remains blocked', flush=True)
        time.sleep(min(60, max(0, until-time.monotonic())))


if __name__ == '__main__':
    main()
