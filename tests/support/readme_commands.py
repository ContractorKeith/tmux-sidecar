"""Execute README bash examples in accept.sh's scratch HOME, never real agent CLIs."""
import errno
import fcntl
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
archive = root / 'tmux-sidecar-v0.1.0-local.tar.gz'
with tarfile.open(archive, 'w:gz') as bundle:
    for source, name in [(binary, 'tmux-sidecar'), ('LICENSE', 'LICENSE'), ('README.md', 'README.md')]:
        bundle.add(source, arcname=name)
home = root / 'home'
home.mkdir()
env = dict(os.environ, HOME=str(home), TMUX=f'{socket},0,0', TERM='xterm-256color',
           CLAUDE_CONFIG_DIR=str(home / '.claude'), CODEX_HOME=str(home / '.codex'),
           GROK_HOME=str(home / '.grok'), OPENCODE_DB=str(home / 'opencode.db'),
           XDG_CONFIG_HOME=str(home / '.config'), XDG_DATA_HOME=str(home / '.local/share'))
env['PATH'] = f"{env['HOME']}/.local/bin:{env['PATH']}"
blocks = re.findall(r'```bash\n(.*?)```', Path('README.md').read_text(), re.S)
for block in blocks:
    # These two blocks configure or launch the real Claude client, outside safe acceptance.
    if 'claude mcp add' in block or 'MCP_TOOL_TIMEOUT=' in block:
        print('README: external Claude example excluded', flush=True)
        continue
    print(f'README: {block.splitlines()[0]}', flush=True)
    cwd = root if 'tar -xzf' in block else Path.cwd()
    if 'tmux-sidecar ui' not in block:
        result = subprocess.run(['bash', '-c' if 'tmux-sidecar doctor' in block else '-ec', block], cwd=cwd, env=env, timeout=180)
        expected = (0, 1) if 'tmux-sidecar doctor' in block else (0,)
        assert result.returncode in expected, (block, result.returncode)
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
