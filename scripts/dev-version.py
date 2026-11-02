#!/usr/bin/env python3
"""taskq.md § 8 item 1: dev build version <next patch>-yymmdd-N.

yymmdd is the UTC build date; N counts that date's builds on this host from 0,
read from the release folders already in <install>/releases.
Usage: dev-version.py Cargo.toml <install>/releases
"""
import datetime
import os
import re
import sys


def version(cargo, releases, now):
    m = re.search(r'^version = "(\d+)\.(\d+)\.(\d+)"$', cargo, re.M)
    day = now.astimezone(datetime.timezone.utc).strftime('%y%m%d')
    used = [int(r[2]) for r in (re.fullmatch(rf'.+-({day})-(\d+)', n) for n in releases) if r]
    return f"{m[1]}.{m[2]}.{int(m[3]) + 1}-{day}-{max(used) + 1 if used else 0}"


if __name__ == '__main__':
    names = os.listdir(sys.argv[2]) if os.path.isdir(sys.argv[2]) else []
    print(version(open(sys.argv[1]).read(), names, datetime.datetime.now(datetime.timezone.utc)))
