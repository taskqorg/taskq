#!/usr/bin/env python3
"""release.yml (taskq.md § 8 item 1): write SHA256SUMS and latest.json for release <version> in <dir>.

Keeps the lines of an existing <dir>/SHA256SUMS (the release's published sums, when an OS is added later)
and adds or replaces one line per taskq-<version>-* file in <dir>; latest.json lists every summed file.
Usage: release-index.py <version> <dir>
"""
import hashlib
import json
import os
import sys


def index(ver, sums_text, files):
    """sums_text: existing SHA256SUMS; files: {name: sha256}. Returns (SHA256SUMS text, latest.json object)."""
    sums = dict(reversed(line.split()) for line in sums_text.splitlines() if line.strip())
    sums.update(files)
    prefix = f'taskq-{ver}-'
    names = sorted(n for n in sums if n.startswith(prefix))
    text = ''.join(f'{sums[n]}  {n}\n' for n in names)
    return text, {'version': ver, 'files': {n[len(prefix):].removesuffix('.exe'): n for n in names}}


if __name__ == '__main__':
    ver, d = sys.argv[1:]
    path = os.path.join(d, 'SHA256SUMS')
    old = open(path).read() if os.path.exists(path) else ''
    files = {n: hashlib.sha256(open(os.path.join(d, n), 'rb').read()).hexdigest()
             for n in os.listdir(d) if n.startswith(f'taskq-{ver}-')}
    text, latest = index(ver, old, files)
    open(path, 'w').write(text)
    json.dump(latest, open(os.path.join(d, 'latest.json'), 'w'), indent=1)
