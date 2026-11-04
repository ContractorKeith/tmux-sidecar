#!/usr/bin/env bash
# scripts/accept.sh — the acceptance for tmux-sidecar. Worker, reviewer and CI run this
# exact script; the orchestrator never retypes it. Keep it under a few minutes.
# Bash 3.2 compatible (macOS): no mapfile, no ${var,,}, no associative arrays.
# Each milestone appends its live checks below; never weaken an earlier one.
set -eu

cd "$(dirname "$0")/.."
command -v tmux >/dev/null || { echo "accept: tmux is required" >&2; exit 1; }

scratch="$(mktemp -d)"
tsock="tsx-accept-$$"
cleanup() {
  if [ -n "${bin:-}" ] && [ -n "${socket:-}" ]; then
    "$bin" --tmux-socket "$socket" daemon stop >/dev/null 2>&1 || true
  fi
  tmux -L "$tsock" kill-server 2>/dev/null || true
  rm -r "$scratch" 2>/dev/null || true
}
trap cleanup EXIT

echo "== isolate"
# All tests and live checks use scratch agent configs, state and tmux sockets.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_STATE_HOME="$scratch/state"
export TMUX_SIDECAR_RUNTIME_DIR="$scratch/run" TMUX_SIDECAR_STATE_DIR="$scratch/data"
export TMUX_TMPDIR="$scratch"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$TMUX_SIDECAR_RUNTIME_DIR" "$TMUX_SIDECAR_STATE_DIR"
chmod 700 "$TMUX_SIDECAR_RUNTIME_DIR" "$TMUX_SIDECAR_STATE_DIR"
export CLAUDE_CONFIG_DIR="$HOME/.claude" CODEX_HOME="$HOME/.codex" GROK_HOME="$HOME/.grok"
export XDG_DATA_HOME="$HOME/.local/share" OPENCODE_DB="$HOME/opencode.db"
unset TMUX TMUX_PANE

echo "== gate"
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
"$bin" hooks install --dry-run > "$scratch/hooks.diff"
test -s "$scratch/hooks.diff"
cmp "$scratch/claude.before" "$HOME/.claude/settings.json"
cmp "$scratch/codex.before" "$HOME/.codex/config.toml"
test ! -e "$HOME/.claude/settings.json.tmux-sidecar.bak"
test ! -e "$HOME/.config/opencode/plugin/tmux-sidecar.js"
"$bin" hook claude </dev/null > "$scratch/hook.stdout" 2> "$scratch/hook.stderr"
test ! -s "$scratch/hook.stdout"
test ! -s "$scratch/hook.stderr"
# M3: a permission fixture puts a live run first in the attention queue.
echo "== state live checks"
cp "$(command -v sleep)" "$scratch/claude"
pane="$(tmux -L "$tsock" new-window -d -P -F '#{pane_id}' "$scratch/claude 120")"
"$bin" --tmux-socket "$socket" list --json >/dev/null
pane_pid="$(tmux -L "$tsock" display -p -t "$pane" '#{pane_pid}')"
server_start="$(tmux -L "$tsock" display -p '#{start_time}')"
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
echo "accept: ok"
