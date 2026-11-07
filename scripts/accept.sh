#!/usr/bin/env bash
# scripts/accept.sh — the acceptance for tmux-sidecar. Worker, reviewer and CI run this
# exact script; the orchestrator never retypes it. Keep it under a few minutes.
# Bash 3.2 compatible (macOS): no mapfile, no ${var,,}, no associative arrays.
# Each milestone appends its live checks below; never weaken an earlier one.
set -eu

cd "$(dirname "$0")/.."
command -v tmux >/dev/null || { echo "accept: tmux is required" >&2; exit 1; }

leak_snapshot="$(mktemp)"
python3 tests/support/leak_check.py snapshot "$leak_snapshot"
scratch="$(mktemp -d /tmp/tsx-accept-XXXXXX)"
tsock="tsx-accept-$$"
cleanup() {
  result=$?
  trap - EXIT
  # Stop every owned server before deleting its socket tree or escalating signals.
  python3 tests/support/leak_check.py cleanup "$scratch" || result=1
  python3 tests/support/leak_check.py check "$leak_snapshot" || result=1
  rm "$leak_snapshot"
  if [ "$result" -eq 0 ]; then echo "accept: ok"; fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo "== isolate"
# All tests and live checks use scratch agent configs, state and tmux sockets.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_STATE_HOME="$scratch/state"
export TMUX_SIDECAR_RUNTIME_DIR="$scratch/run" TMUX_SIDECAR_STATE_DIR="$scratch/data"
export TMUX_TMPDIR="$scratch" TMPDIR="$scratch/tmp"
mkdir -p "$TMPDIR"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$TMUX_SIDECAR_RUNTIME_DIR" "$TMUX_SIDECAR_STATE_DIR"
chmod 700 "$TMUX_SIDECAR_RUNTIME_DIR" "$TMUX_SIDECAR_STATE_DIR"
export CLAUDE_CONFIG_DIR="$HOME/.claude" CODEX_HOME="$HOME/.codex" GROK_HOME="$HOME/.grok"
export XDG_DATA_HOME="$HOME/.local/share" OPENCODE_DB="$HOME/opencode.db"
mkdir -p "$XDG_CONFIG_HOME/tmux-sidecar" "$HOME/Library/Application Support/tmux-sidecar"
printf '[notify]\ndesktop=false\ntmux=false\n' > "$XDG_CONFIG_HOME/tmux-sidecar/config.toml"
cp "$XDG_CONFIG_HOME/tmux-sidecar/config.toml" "$HOME/Library/Application Support/tmux-sidecar/config.toml"
unset TMUX TMUX_PANE

echo "== gate"
python3 tests/support/leak_check_test.py
cargo fmt --all --check
rustfmt --edition 2024 --check tests/support/fake_agent.rs
cargo clippy --locked --all-targets -- -D warnings
TMUX_SIDECAR_E2E=1 cargo test --locked
test "$(cargo tree -d --locked 2>/dev/null | grep -c 'libsqlite3-sys')" -eq 0

echo "== build"
cargo build --release --locked
bin="${ACCEPT_BIN:-$PWD/target/release/tmux-sidecar}"
"$bin" --version

echo "== e2e (real tmux)"
TMUX_SIDECAR_E2E=1 cargo test --locked --test e2e

echo "== live checks"
# M1: the script owns this server and daemon; the EXIT trap handles failures too.
tmux -L "$tsock" -f /dev/null new-session -d -s fixture 'sleep 120'
socket="$(tmux -L "$tsock" display -p '#{socket_path}')"
printf '%s' "$socket" > "$scratch/tmux.socket"
tmux -L "$tsock" display -p '#{pid}' > "$scratch/tmux.pid"
status=0
"$bin" --tmux-socket "$socket" daemon status >/dev/null 2>&1 || status=$?
test "$status" -eq 6
"$bin" --tmux-socket "$socket" list --json
"$bin" --tmux-socket "$socket" daemon status --json
daemon_pid="$(cat "$TMUX_SIDECAR_RUNTIME_DIR/daemon.pid")"
"$bin" --tmux-socket "$socket" daemon stop
test ! -e "$TMUX_SIDECAR_RUNTIME_DIR/sock"
if kill -0 "$daemon_pid" 2>/dev/null; then
  # A just-exited orphan can briefly be a zombie; it is no longer a daemon.
  if ps -p "$daemon_pid" -o args= | grep -q 'tmux-sidecar.*daemon.*run'; then
    echo "accept: daemon still running" >&2
    exit 1
  fi
fi

# M2: dry-run must leave the scratch agent config tree unchanged.
echo "== hook live checks"
mkdir -p "$HOME/.claude" "$HOME/.codex"
printf '%s\n' '{"user": true}' > "$HOME/.claude/settings.json"
printf '%s\n' '# user comment' 'model = "fixture"' > "$HOME/.codex/config.toml"
cp "$HOME/.claude/settings.json" "$scratch/claude.before"
cp "$HOME/.codex/config.toml" "$scratch/codex.before"
"$bin" hooks install --agent claude --agent codex --agent grok --agent opencode --dry-run > "$scratch/hooks.diff"
test -s "$scratch/hooks.diff"
cmp "$scratch/claude.before" "$HOME/.claude/settings.json"
cmp "$scratch/codex.before" "$HOME/.codex/config.toml"
test ! -e "$HOME/.claude/settings.json.tmux-sidecar.bak"
test ! -e "$XDG_CONFIG_HOME/opencode/plugins/tmux-sidecar.js"
"$bin" hook claude </dev/null > "$scratch/hook.stdout" 2> "$scratch/hook.stderr"
test ! -s "$scratch/hook.stdout"
test ! -s "$scratch/hook.stderr"
# M3: a permission fixture puts a live run first in the attention queue.
echo "== state live checks"
rustc --edition 2024 tests/support/fake_agent.rs -o "$scratch/claude"
tmux -L "$tsock" new-window -d -t fixture -n state "$scratch/claude"
"$bin" --tmux-socket "$socket" list --json >/dev/null
pane_info="$(tmux -L "$tsock" list-panes -t fixture:state -F '#{pane_id} #{pane_pid} #{start_time}')"
read -r pane pane_pid server_start <<PANE_INFO
$pane_info
PANE_INFO
test -n "$server_start" || { echo "accept: incomplete pane metadata: $pane_info" >&2; exit 1; }
python3 - "$TMUX_SIDECAR_RUNTIME_DIR/sock" "$pane_pid" "$server_start" "$pane" <<'PYLIVE'
import json, socket, sys, time
payload = json.load(open('tests/fixtures/hooks/claude/Notification-permission_prompt.json'))
frame = dict(jsonrpc='2.0', id=1, method='signal', params=dict(
    server_start=int(sys.argv[3]), pane_id=sys.argv[4], agent='claude', agent_pid=int(sys.argv[2]),
    at=int(time.time()*1000), nonce='0123456789abcdef0123456789abcdef', source='hook:claude',
    event='Notification', detail='permission_prompt', cwd='/', payload=payload))
with socket.socket(socket.AF_UNIX) as client:
    client.settimeout(2)
    client.connect(sys.argv[1])
    client.sendall((json.dumps(frame)+'\n').encode())
    assert 'result' in json.loads(client.makefile().readline())
PYLIVE
"$bin" --tmux-socket "$socket" attention --json > "$scratch/attention.json"
python3 - "$scratch/attention.json" <<'PYLIVE'
import json, sys
assert json.load(open(sys.argv[1]))[0]['state'] == 'blocked'
PYLIVE
# M4: guarded send, daemon-enforced wait timeout and missing selectors.
status=0
"$bin" --tmux-socket "$socket" send "$pane" refused >/dev/null 2>&1 || status=$?
test "$status" -eq 3
status=0
"$bin" --tmux-socket "$socket" wait "$pane" --until waiting --timeout 1 >/dev/null 2>&1 || status=$?
test "$status" -eq 4
status=0
"$bin" --tmux-socket "$socket" history r999999 >/dev/null 2>&1 || status=$?
test "$status" -eq 5
# M6: keep stdin open through initialize and tools/list; EOF cancels SDK requests.
echo "== MCP live checks"
python3 - "$bin" "$socket" <<'PYMCP'
import json, selectors, subprocess, sys
child = subprocess.Popen([sys.argv[1], '--tmux-socket', sys.argv[2], 'mcp'],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
try:
    def request(frame):
        child.stdin.write(json.dumps(frame) + '\n')
        child.stdin.flush()
        with selectors.DefaultSelector() as poll:
            poll.register(child.stdout, selectors.EVENT_READ)
            assert poll.select(5), 'MCP response deadline exceeded'
        reply = json.loads(child.stdout.readline())
        assert reply['id'] == frame['id'] and 'error' not in reply, reply
        return reply['result']
    request(dict(jsonrpc='2.0', id=1, method='initialize', params=dict(
        protocolVersion='2024-11-05', capabilities={}, clientInfo=dict(name='accept', version='1'))))
    child.stdin.write(json.dumps(dict(jsonrpc='2.0', method='notifications/initialized')) + '\n')
    child.stdin.flush()
    tools = request(dict(jsonrpc='2.0', id=2, method='tools/list', params={}))['tools']
    assert {tool['name'] for tool in tools} == {'list_runs', 'attention', 'run_history', 'wait_for_state'}
finally:
    child.kill()
    child.wait(timeout=5)
PYMCP


# M5: terminal fixtures, decoder, notifier policy, and real-tmux Enter run in the gate above.
echo "== ui live checks"
"$bin" ui --help | grep -q "Open the terminal interface"
grep -q "display-popup.*ui" "$scratch/hooks.diff"
# M7: missing hooks fail, installed hooks pass even when unused stores are absent.
echo "== doctor live checks"
status=0
"$bin" --tmux-socket "$socket" doctor --agent claude --agent codex --agent grok --agent opencode --json > "$scratch/doctor-before.json" || status=$?
test "$status" -eq 1
"$bin" hooks install --agent claude --agent codex --agent grok --agent opencode
status=0
"$bin" --tmux-socket "$socket" doctor --json > "$scratch/doctor-after.json" || status=$?
python3 - "$scratch/doctor-before.json" "$scratch/doctor-after.json" <<'PYDOCTOR'
import json, sys
before, after = [json.load(open(p)) for p in sys.argv[1:]]
assert all(c['status'] == 'fail' for c in before if c['name'].startswith('hooks '))
assert len([c for c in after if c['name'].startswith('hooks ') and c['status'] == 'ok']) == 4
PYDOCTOR
# Execute the README's safe bash blocks verbatim, including the UI under a PTY.
python3 tests/support/readme_commands.py "$bin" "$socket" "$scratch"
