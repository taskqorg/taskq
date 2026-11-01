#!/usr/bin/env python3
"""#2151: owned process fixtures only; no client, model, slot or host-policy operation."""
import ctypes
import errno
import json
import os
from pathlib import Path
import select
import signal
import sys
import time

BUDGET = 10
LIB = ctypes.CDLL('/usr/lib/libsandbox.dylib', use_errno=True) if sys.platform == 'darwin' else None


def read(fd):
    if not select.select([fd], [], [], BUDGET)[0]:
        raise TimeoutError('owned fixture pipe deadline')
    value = os.read(fd, 4096)
    if not value:
        raise RuntimeError('owned fixture pipe closed without receipt')
    return json.loads(value)


def send(fd, value):
    value = json.dumps(value).encode() + b'\n'
    assert len(value) < 4096
    assert os.write(fd, value) == len(value)


def wait(pid):
    until = time.monotonic() + BUDGET
    while time.monotonic() < until:
        child, status = os.waitpid(pid, os.WNOHANG)
        if child:
            assert os.waitstatus_to_exitcode(status) == 0, 'owned fixture failed'
            return
        select.select([], [], [], 0.01)
    # This is our unreaped direct child; its PID cannot have been reused.
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    raise TimeoutError('owned fixture exit deadline')


def stamp(pid):
    class Unique(ctypes.Structure):
        _fields_ = [('uuid', ctypes.c_ubyte * 16), ('id', ctypes.c_uint64),
                    ('parent', ctypes.c_uint64), ('version', ctypes.c_int32),
                    ('parent_version', ctypes.c_int32), ('reserved', ctypes.c_uint64 * 2)]
    value = Unique()
    libproc = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
    n = libproc.proc_pidinfo(pid, 17, ctypes.c_uint64(0), ctypes.byref(value), ctypes.sizeof(value))
    assert n == 56 and value.id > 0, 'owned native identity unavailable'
    return value.id


def descendant(ready, release):
    # The controller owns this private pipe; no exit depends on a model or a service.
    send(ready, {'pid': os.getpid(), 'unique': stamp(os.getpid()),
                 'group': os.getpgrp(), 'session': os.getsid(0)})
    os.close(ready)
    if not select.select([release], [], [], BUDGET)[0]:
        os._exit(4)
    os.read(release, 1)
    os.close(release)
    os._exit(0)


def escape(mode):
    ready_r, ready_w = os.pipe()
    release_r, release_w = os.pipe()
    root = os.fork()
    if root == 0:
        try:
            os.close(ready_r)
            os.close(release_w)
            os.setpgid(0, 0)
            if mode == 'setsid':
                child = os.fork()
                if child == 0:
                    os.setsid()
                    descendant(ready_w, release_r)
            else:
                for fd in (ready_w, release_r):
                    os.set_inheritable(fd, True)
                os.posix_spawn(sys.executable,
                               [sys.executable, str(Path(__file__).resolve()), '--descendant',
                                str(ready_w), str(release_r)], os.environ, setsid=True)
            os.close(ready_w)
            os.close(release_r)
            os._exit(0)
        except BaseException:
            os._exit(3)
    os.close(ready_w)
    os.close(release_r)
    observed = None
    reaped = False
    try:
        observed = read(ready_r)
        wait(root)
        reaped = True
        assert stamp(observed['pid']) == observed['unique']
        assert observed['group'] != root and observed['session'] == observed['pid']
        group_absent = False
        try:
            os.killpg(root, 0)  # Read only. Never signal a process group during the probe.
        except ProcessLookupError:
            group_absent = True
        assert group_absent, 'fixture original group must be empty'
        result = {'mode': mode, 'root_reaped': True, 'original_group_absent': True,
                  'exact_descendant_live': True, 'descendant_escaped_group': True,
                  'root_or_group_absence_proves_completion': False}
    finally:
        os.close(ready_r)
        os.close(release_w)  # EOF tells the exact owned descendant to finish.
        if not reaped:
            wait(root)
        if observed:
            until = time.monotonic() + BUDGET
            while time.monotonic() < until:
                try:
                    current = stamp(observed['pid'])
                except AssertionError:
                    break
                if current != observed['unique']:
                    break
                select.select([], [], [], 0.01)
            else:
                raise TimeoutError('owned descendant teardown unconfirmed')
    return result


def tracking():
    results = []
    for name, flags in [('NOTE_FORK|NOTE_EXIT', select.KQ_NOTE_FORK | select.KQ_NOTE_EXIT),
                        ('NOTE_FORK|NOTE_EXIT|NOTE_TRACK', select.KQ_NOTE_FORK | select.KQ_NOTE_EXIT | 1)]:
        queue = select.kqueue()
        try:
            queue.control([select.kevent(os.getpid(), filter=select.KQ_FILTER_PROC,
                           flags=select.KQ_EV_ADD | select.KQ_EV_ENABLE | select.KQ_EV_CLEAR,
                           fflags=flags)], 0, 0)
            results.append({'flags': name, 'state': 'available'})
        except OSError as error:
            results.append({'flags': name, 'state': 'refused', 'errno': error.errno})
        finally:
            queue.close()
    return results


def profile_candidate():
    profile = '(version 1)(allow default)\n' + Path(__file__).with_name('group-narrowing.sb').read_text()
    ready_r, ready_w = os.pipe()
    child = os.fork()
    if child == 0:
        os.close(ready_r)
        error = ctypes.c_char_p()
        LIB.sandbox_init.argtypes = [ctypes.c_char_p, ctypes.c_uint64, ctypes.POINTER(ctypes.c_char_p)]
        rc = LIB.sandbox_init(profile.encode(), 0, ctypes.byref(error))
        if rc != 0:
            send(ready_w, {'state': 'sandbox-blocked', 'operation': 'sandbox_init', 'return': rc,
                          'error': error.value.decode() if error.value else 'unknown'})
            os._exit(0)
        observations = []
        for name, action in [('setsid', lambda: os.setsid()), ('setpgid', lambda: os.setpgid(0, 0)),
                             ('posix_spawn', lambda: os.posix_spawn('/usr/bin/true', ['true'], os.environ))]:
            try:
                result = action()
            except OSError as error:
                observations.append({'operation': name, 'errno': error.errno})
            else:
                if name == 'posix_spawn':
                    os.waitpid(result, 0)
                observations.append({'operation': name, 'unexpected_success': True})
        send(ready_w, {'state': 'observed', 'calls': observations})
        os._exit(0)
    os.close(ready_w)
    try:
        observed = read(ready_r)
        wait(child)
    finally:
        os.close(ready_r)
    if observed['state'] == 'observed':
        assert observed['calls'] == [{'operation': 'setsid', 'errno': errno.EPERM},
                                     {'operation': 'setpgid', 'errno': errno.EPERM},
                                     {'operation': 'posix_spawn', 'errno': errno.ENOSYS}], observed
    return observed


def main():
    if len(sys.argv) == 4 and sys.argv[1] == '--descendant':
        descendant(int(sys.argv[2]), int(sys.argv[3]))
    if sys.platform != 'darwin':
        print(json.dumps({'state': 'unsupported', 'reason': 'macOS process investigation only'}))
        return
    print(json.dumps({'schema': 'taskq.scope-investigation/1', 'tracking': tracking(),
                      'escape_traces': [escape('setsid'), escape('posix_spawn')],
                      'group_narrowing': profile_candidate()}, indent=2))


if __name__ == '__main__':
    main()
