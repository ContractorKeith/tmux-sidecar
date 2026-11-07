"""Snapshot real-machine boundaries before isolation; reject additions after cleanup."""
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import time


def paths(roots):
    found = set()
    for root in roots:
        root = Path(root)
        if not root.exists():
            continue
        found.add(str(root))
        if root.is_dir() and not root.is_symlink():
            for parent, dirs, files in os.walk(root, followlinks=False):
                found.update(str(Path(parent) / name) for name in dirs + files)
    return found


def processes():
    result = subprocess.run(['/bin/ps', '-A', '-o', 'pid=,uid=,stat=,comm='],
                            capture_output=True, text=True, check=True, timeout=5)
    found = set()
    for line in result.stdout.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4 and int(fields[1]) == os.getuid() and 'Z' not in fields[2]:
            name = Path(fields[3]).name.split(':')[0]
            if name in ('tmux', 'tmux-sidecar'):
                found.add(fields[0] + ' ' + fields[3])
    return found


def roots():
    home = Path.home()
    state = Path(os.environ.get('XDG_STATE_HOME', home / '.local/state'))
    socket_dirs = {Path('/tmp') / f'tmux-{os.getuid()}',
                   Path(os.environ.get('TMUX_TMPDIR', '/tmp')) / f'tmux-{os.getuid()}'}
    return dict(socket_dirs=sorted(map(str, socket_dirs)), state_dirs=list(map(str, {
        state / 'tmux-sidecar', home / '.local/state/tmux-sidecar',
        home / 'Library/Application Support/tmux-sidecar'})))


def snapshot(boundaries):
    sockets = [entry for parent in boundaries['socket_dirs']
               if Path(parent).exists() for entry in Path(parent).glob('tsx-*')]
    return dict(paths=sorted(paths(sockets + list(Path(boundaries.get('scratch_dir', '/tmp')).glob('tsx-*')) +
                                  [Path(p) for p in boundaries['state_dirs']])),
                processes=sorted(processes()))


def cleanup(root):
    root = Path(root).resolve()
    pids = set()
    for name in ('tmux.pid', 'daemon.pid'):
        for record in root.rglob(name):
            try:
                pid = int(record.read_text())
                if pid > 1:
                    pids.add(pid)
            except (OSError, ValueError):
                pass
    sockets = set(root.glob('**/tmux-*/tsx-*'))
    for record in root.rglob('tmux.socket'):
        sockets.add(Path(record.read_text().strip()))
    # Every server gets a graceful request before any signal is sent.
    for socket in sockets:
        if not socket.resolve().is_relative_to(root):
            raise AssertionError(f'cleanup socket escaped scratch tree: {socket}')
        env = dict(os.environ, HOME=str(root), TMUX_TMPDIR=str(root),
                   TMUX_SIDECAR_RUNTIME_DIR=str(root / 'run'),
                   TMUX_SIDECAR_STATE_DIR=str(root / 'state'))
        try:
            subprocess.run(['tmux', '-S', str(socket), 'kill-server'], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=2)
        except (OSError, subprocess.TimeoutExpired):
            pass
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in pids:
            try:
                os.kill(pid, signal.SIGCONT)
                os.kill(pid, sig)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 1
        while time.monotonic() < deadline:
            live = []
            for pid in pids:
                try:
                    os.kill(pid, 0)
                    live.append(pid)
                except ProcessLookupError:
                    pass
            if not live:
                break
            time.sleep(.02)
    def repair(function, path, _error):
        os.chmod(path, stat.S_IRWXU)
        function(path)
    shutil.rmtree(root, onerror=repair)


if __name__ == '__main__':
    action, filename = sys.argv[1:]
    file = Path(filename)
    if action == 'snapshot':
        boundary = roots()
        file.write_text(json.dumps(dict(boundaries=boundary, before=snapshot(boundary))))
    elif action == 'check':
        baseline = json.loads(file.read_text())
        after = snapshot(baseline['boundaries'])
        added = {key: sorted(set(after[key]) - set(baseline['before'][key]))
                 for key in ('paths', 'processes')}
        if any(added.values()):
            raise SystemExit('accept: leaked real-machine resources:\n' + json.dumps(added, indent=2))
        print('accept: leak check ok')
    elif action == 'cleanup':
        cleanup(file)
    else:
        raise SystemExit(f'unknown action: {action}')
