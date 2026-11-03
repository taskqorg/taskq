#!/usr/bin/env python3
"""taskq.md § 8 item 1: dev build version <next patch>-yymmdd-N.

The patch follows the greater of the Cargo.toml version and the highest vX.Y.Z release tag, since releases take
their version from the tag, not from a Cargo.toml bump. yymmdd is the UTC build date; N counts that date's builds
on this host from 0, read from the release folders already in <install>/releases.
Usage: dev-version.py Cargo.toml <install>/releases [tag..]
"""
import datetime
import os
import re
import sys


def version(cargo, releases, now, tags=()):
    m = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"$', cargo, re.M)
    m = max([tuple(map(int, m.groups()))] + [tuple(map(int, t.groups())) for t in (re.fullmatch(r'v(\d+)\.(\d+)\.(\d+)', t) for t in tags) if t])
    day = now.astimezone(datetime.timezone.utc).strftime('%y%m%d')
    used = [int(r[2]) for r in (re.fullmatch(rf'.+-({day})-(\d+)', n) for n in releases) if r]
    return f"{m[0]}.{m[1]}.{m[2] + 1}-{day}-{max(used) + 1 if used else 0}"


if __name__ == '__main__':
    names = os.listdir(sys.argv[2]) if os.path.isdir(sys.argv[2]) else []
    print(version(open(sys.argv[1]).read(), names, datetime.datetime.now(datetime.timezone.utc), sys.argv[3:]))
