"""Reject processes and paths tied to acceptance’s scratch directory after cleanup."""
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
        if not root.exists() and not root.is_symlink():
            continue
        found.add(str(root))
        if root.is_dir() and not root.is_symlink():
            for parent, dirs, files in os.walk(root, followlinks=False):
                found.update(str(Path(parent) / name) for name in dirs + files)
    return found


def processes(scope=None):
    result = subprocess.run(['/bin/ps', '-A', '-o', 'pid=,uid=,stat=,comm='],
                            capture_output=True, text=True, check=True, timeout=5)
    found = set()
    for line in result.stdout.splitlines():
        fields = line.split(None, 3)
        if len(fields) == 4 and int(fields[1]) == os.getuid() and 'Z' not in fields[2]:
            name = Path(fields[3]).name.split(':')[0]
            if name in ('tmux', 'tmux-sidecar'):
                pid = fields[0]
                if scope and not references(pid, scope):
                    continue
                found.add(pid + ' ' + fields[3])
    return found


def references(pid, scope):
    scope = os.fsencode(str(Path(scope).resolve()))
    if sys.platform.startswith('linux'):
        try:
            proc = Path('/proc') / pid
            values = [proc.joinpath('cmdline').read_bytes(),
                      proc.joinpath('environ').read_bytes(),
                      os.fsencode(os.readlink(proc / 'cwd'))]
        except OSError:
            return False  # The process exited during the snapshot.
        return any(scope in value for value in values)
    # Darwin ps -E includes the environment, including HOME and TMUX_TMPDIR.
    result = subprocess.run(['/bin/ps', '-E', '-ww', '-p', pid, '-o', 'command='],
                            capture_output=True, check=False, timeout=5)
    if scope in result.stdout:
        return True
    result = subprocess.run(['/usr/sbin/lsof', '-a', '-p', pid, '-d', 'cwd', '-Fn'],
                            capture_output=True, check=False, timeout=5)
    return scope in result.stdout


def snapshot(boundaries):
    scope = boundaries['scratch_dir']
    return dict(paths=sorted(paths([scope])), processes=sorted(processes(scope)))


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
    action, filename = sys.argv[1:3]
    file = Path(filename)
    if action == 'snapshot':
        boundary = dict(scratch_dir=str(Path(sys.argv[3]).resolve()))
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
