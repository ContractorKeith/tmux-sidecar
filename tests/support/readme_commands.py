"""Execute README bash examples in accept.sh's scratch HOME, never real agent CLIs."""
import errno
import fcntl
import hashlib
import os
from pathlib import Path
import pty
import re
import select
import subprocess
import struct
import termios
import sys
import tarfile
import time

binary, socket, scratch = sys.argv[1:]
root = Path(scratch) / 'readme'
root.mkdir()
# Supply the not-yet-published archive using the binary acceptance just built.
version = subprocess.run([binary, '--version'], capture_output=True, text=True, check=True, timeout=5).stdout.split()[-1]
targets = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin']
target = targets[2] if sys.platform == 'darwin' else targets[0]
archive = root / f'tmux-sidecar-v{version}-{target}.tar.gz'
with tarfile.open(archive, 'w:gz') as bundle:
    for source, name in [(binary, 'tmux-sidecar'), ('LICENSE', 'LICENSE'), ('README.md', 'README.md')]:
        bundle.add(source, arcname=name)
# Match the real aggregate manifest: no ./ prefix, with other platforms absent locally.
(root / 'SHA256SUMS').write_text(''.join(
    f"{hashlib.sha256(archive.read_bytes()).hexdigest() if item == target else '0' * 64}  tmux-sidecar-v{version}-{item}.tar.gz\n"
    for item in targets))
home = root / 'home'
home.mkdir()
# Make doctor examples healthy without invoking agent CLIs or desktop notifications.
for relative in ['.claude/projects', '.codex/sessions', '.grok/sessions']:
    (home / relative).mkdir(parents=True)
for relative in ['.grok/sessions/session_search.sqlite', 'opencode.db']:
    (home / relative).touch()
notifiers = root / 'bin'
notifiers.mkdir()
for name in ['notify-send', 'osascript']:
    path = notifiers / name
    path.write_text('#!/bin/sh\nexit 0\n')
    path.chmod(0o700)
env = dict(os.environ, HOME=str(home), TMUX=f'{socket},0,0', TERM='xterm-256color',
           CLAUDE_CONFIG_DIR=str(home / '.claude'), CODEX_HOME=str(home / '.codex'),
           GROK_HOME=str(home / '.grok'), OPENCODE_DB=str(home / 'opencode.db'),
           XDG_CONFIG_HOME=str(home / '.config'), XDG_DATA_HOME=str(home / '.local/share'))
env['PATH'] = f"{notifiers}:{env['HOME']}/.local/bin:{env['PATH']}"
blocks = re.findall(r'```bash\n(.*?)```', Path('README.md').read_text(), re.S)
for block in blocks:
    # These two blocks configure or launch the real Claude client, outside safe acceptance.
    if 'claude mcp add' in block or 'MCP_TOOL_TIMEOUT=' in block:
        print('README: external Claude example excluded', flush=True)
        continue
    print(f'README: {block.splitlines()[0]}', flush=True)
    cwd = root if 'tar -xzf' in block or 'SHA256SUMS' in block else Path.cwd()
    if 'tmux-sidecar ui' not in block:
        result = subprocess.run(['bash', '-ec', block], cwd=cwd, env=env, timeout=180)
        assert result.returncode == 0, (block, result.returncode)
        if 'SHA256SUMS' in block:
            original = archive.read_bytes()
            try:
                archive.write_bytes(original + b'corrupted')
                bad = subprocess.run(['bash', '-ec', block], cwd=cwd, env=env,
                                     capture_output=True, timeout=5)
                assert bad.returncode != 0, 'README checksum command accepted corrupt archive'
                print('README: corrupt archive rejected', flush=True)
            finally:
                archive.write_bytes(original)
        continue
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
    child = subprocess.Popen(['bash', '-ec', block], cwd=cwd, env=env,
                             stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
    os.close(slave)
    captured = bytearray()
    quit_sent = False
    deadline = time.monotonic() + 10
    try:
        while child.poll() is None and time.monotonic() < deadline:
            if select.select([master], [], [], 0.1)[0]:
                try:
                    captured.extend(os.read(master, 65536))
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
            if not quit_sent and (b'Attention' in captured or b'attention' in captured):
                quit_sent = True
                try:
                    os.write(master, b'q')
                except OSError as error:
                    # BSD reports EIO if the slave closed between poll and write.
                    # The exit-status assertion below still catches UI failures.
                    if error.errno != errno.EIO:
                        raise
        assert child.wait(timeout=1) == 0, captured.decode(errors='replace')
        assert b'not a terminal' not in captured
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=5)
        os.close(master)

# A tmux configuration block is safe when sourced into the isolated server only.
for index, block in enumerate(re.findall(r'```tmux\n(.*?)```', Path('README.md').read_text(), re.S)):
    config = root / f'tmux-{index}.conf'
    config.write_text(block)
    subprocess.run(['tmux', '-S', socket, 'source-file', str(config)],
                   env=env, check=True, timeout=5)
    print('README: popup binding sourced into isolated server', flush=True)
