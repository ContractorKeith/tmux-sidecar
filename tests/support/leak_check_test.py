"""Negative checks for each acceptance boundary, without writing to real paths."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
sys.dont_write_bytecode = True
from leak_check import snapshot

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    for name in ['sockets', 'scratch', 'state']:
        (root / name).mkdir()
    boundary = dict(socket_dirs=[str(root / 'sockets')],
                    state_dirs=[str(root / 'state')], scratch_dir=str(root / 'scratch'))
    env = dict(os.environ, HOME=str(root), TMUX_TMPDIR=str(root),
               TMUX_SIDECAR_RUNTIME_DIR=str(root / 'run'), TMUX_SIDECAR_STATE_DIR=str(root / 'state'))
    before = snapshot(boundary)
    baseline = root / 'baseline.json'
    baseline.write_text(json.dumps(dict(boundaries=boundary, before=before)))
    command = [sys.executable, str(Path(__file__).with_name('leak_check.py')), 'check', str(baseline)]
    for path in [root / 'sockets/tsx-leak', root / 'scratch/tsx-leak', root / 'state/new-key']:
        path.mkdir()
        result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=5)
        assert result.returncode != 0 and str(path) in result.stderr, result
        path.rmdir()
    subprocess.run(command, env=env, check=True, timeout=5)
    print('accept: leak detector rejects new sockets, scratch directories and real-state entries')
