#!/usr/bin/env python3
"""Unexecuted #2151 launchd candidate. Never registers or dispatches a job itself.

Only --self-check is nonlaunching. Native modes require the separately reviewed
transient-job experiment; these fixture receipts never authorize native input.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import plistlib
import sys
import tempfile
import time
import uuid

from scope_probe import accounting, coalition, stamp

BUDGET = 30


def save(path, value):
    # A generation never overwrites an existing receipt or launch intention.
    with path.open('x') as stream:
        json.dump(value, stream)
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def boot():
    # Read in-process: even a sysctl utility would fork before the binding barrier.
    lib = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    query = lib.sysctlbyname
    query.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t),
                      ctypes.c_void_p, ctypes.c_size_t]
    query.restype = ctypes.c_int
    value = ctypes.create_string_buffer(128)
    length = ctypes.c_size_t(len(value))
    assert query(b'kern.bootsessionuuid', value, ctypes.byref(length), None, 0) == 0
    assert length.value == 37 and value.raw[36] == 0, 'boot identity unavailable'
    return str(uuid.UUID(value.value.decode('ascii')))


def identity():
    pid = os.getpid()
    return {'pid': pid, 'unique': str(stamp(pid)), 'coalition': str(coalition(pid)),
            'boot': boot()}


def await_marker(path):
    deadline = time.monotonic() + BUDGET
    while time.monotonic() < deadline:
        if path.is_file():
            return True
        time.sleep(0.02)
    return False


def prepare(directory):
    directory.mkdir(mode=0o700)
    directory = directory.resolve(strict=True)
    source = Path(__file__).resolve(strict=True)
    program = Path(sys.executable).resolve(strict=True)
    label = 'org.taskq.fixture.2151.' + uuid.uuid4().hex
    plist = {'Label': label, 'Program': str(program),
             'ProgramArguments': [str(program), str(source), '--root', str(directory)],
             'RunAtLoad': False, 'KeepAlive': False, 'LaunchOnlyOnce': True,
             'AbandonProcessGroup': True}
    with (directory / 'job.plist').open('xb') as stream:
        plistlib.dump(plist, stream)
        stream.flush()
        os.fsync(stream.fileno())
    save(directory / 'prepared.json', {
        'label': label, 'domain': 'user/' + str(os.getuid()), 'boot': boot(),
        'files': {str(path): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in (source, source.with_name('scope_probe.py'), program,
                               directory / 'job.plist')},
        'controller_coalition': str(coalition(os.getpid()))})


def root(directory):
    expected = json.loads((directory / 'prepared.json').read_text())
    actual = identity()
    assert actual['boot'] == expected['boot']
    assert actual['coalition'] != expected['controller_coalition'], 'scope is not isolated'
    assert all(hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest
               for path, digest in expected['files'].items()), 'candidate changed'
    save(directory / 'root.json', actual)
    if not await_marker(directory / 'go'):
        return  # Before GO, no fork/native payload is permitted.
    save(directory / 'dispatch.json', {'one_fixture_spawn': True})
    # Preserve the exact ordinary native API rejected by the former candidate.
    os.posix_spawn(sys.executable,
                   [sys.executable, str(Path(__file__).resolve()), '--leaf', str(directory)],
                   os.environ, setsid=True)
    # launchd reaps this root; the leaf deliberately survives in a new session.


def leaf(directory):
    actual = identity()
    original = json.loads((directory / 'root.json').read_text())
    assert actual['boot'] == original['boot'] and actual['coalition'] == original['coalition']
    assert os.getsid(0) == os.getpid()
    save(directory / 'leaf.json', actual)
    stopped = await_marker(directory / 'stop')
    save(directory / 'leaf-finished.json', {'cause': 'controller' if stopped else 'watchdog'})


def inspect(directory):
    original = json.loads((directory / 'root.json').read_text())
    assert original['boot'] == boot(), 'different boot; fixture readback refused'
    result = accounting(int(original['coalition']))
    # Missing coalition is UNKNOWN, never an empty-counter observation.
    live = []
    for name in ('root', 'leaf'):
        path = directory / (name + '.json')
        if not path.exists():
            live.append(name + ': not bound')
            continue
        expected = json.loads(path.read_text())
        try:
            current = stamp(expected['pid'])
        except AssertionError:
            live.append(name + ': absent-or-unreadable')
        else:
            exact = str(current) == expected['unique']
            if exact:
                assert str(coalition(expected['pid'])) == original['coalition']
                assert str(stamp(expected['pid'])) == expected['unique'], 'identity changed during readback'
            live.append(name + (': exact live' if exact else ': PID reused'))
    print(json.dumps({'accounting': result, 'fixtures': live,
                      'whole_operation_completion': 'NOT_QUALIFIED',
                      'external_effect_completion': 'NOT_CLAIMED'}))


def self_check():
    # Validate the experiment's dispatch surface without calling launchctl.
    if sys.platform != 'darwin':
        print('NOT_RUN: macOS-only fixture plan')
        return
    with tempfile.TemporaryDirectory(prefix='taskq-2151-plan-') as tmp:
        directory = Path(tmp) / 'fixture'
        prepare(directory)
        plist = plistlib.loads((directory / 'job.plist').read_bytes())
        assert set(plist) == {'Label', 'Program', 'ProgramArguments', 'RunAtLoad',
                              'KeepAlive', 'LaunchOnlyOnce', 'AbandonProcessGroup'}
        assert not plist['RunAtLoad'] and not plist['KeepAlive'] and plist['LaunchOnlyOnce']
        assert plist['ProgramArguments'][-2:] == ['--root', str(directory.resolve())]
        try:
            root(directory)
        except AssertionError as error:
            assert str(error) == 'scope is not isolated'
        else:
            raise AssertionError('shared-scope root unexpectedly admitted')
        assert not any((directory / name).exists()
                       for name in ('root.json', 'dispatch.json', 'leaf.json'))
        try:
            prepare(directory)
        except FileExistsError:
            pass
        else:
            raise AssertionError('generation unexpectedly replaced')
    print('PASS: fixture plan only; no launchctl, native job, scope completion or runtime qualification')


if __name__ == '__main__':
    if sys.argv[1:] == ['--self-check']:
        self_check()
    else:
        assert len(sys.argv) == 3 and sys.platform == 'darwin'
        mode, directory = sys.argv[1], Path(sys.argv[2])
        assert directory.is_absolute()
        {'--prepare': prepare, '--root': root, '--leaf': leaf, '--inspect': inspect}[mode](directory)
