# tmux-sidecar

Know which of your coding agents needs you. `tmux-sidecar` sits beside tmux and tracks every
Claude Code, Codex, Grok and OpenCode run in your panes: working, waiting for a prompt,
blocked on an approval, idle, errored or done.

- An attention queue: blocked first, then errors, then agents waiting for their next prompt.
- A status UI you put in any pane or pop up with a keybind, and a desktop notification when
  an agent is blocked.
- `tmux-sidecar send %7 "run the tests"` types into a waiting agent without attaching, and
  refuses while it is working.
- `tmux-sidecar wait %7 --until waiting` for scripts, `--json` on every read command.
- An MCP server so one agent can watch others (and, if you allow it, prompt them), and a
  versioned JSON-RPC socket for other tools.

State comes from the agents' own hooks (`tmux-sidecar hooks install`), with transcript reading
from [session-board](https://github.com/ContractorKeith/session-board) as a fallback. Nothing
reads your screen.

**Status:** v1 feature-complete, untagged, pending skeptic review. See `docs/PLAN.md`. Linux and macOS, tmux ≥ 3.3.

## Install

Requires tmux ≥ 3.3. Release archives support Linux x86_64 and aarch64, and macOS arm64. Once v1 is
tagged, download the archive for your platform and SHA256SUMS from
[Releases](https://github.com/ContractorKeith/tmux-sidecar/releases). There is no published v1
release yet. In the download directory, keep the selected platform's archive and SHA256SUMS, then
verify before extracting:

```bash
if [ "$(uname -s)" = Darwin ]; then
  grep 'tmux-sidecar-v.*-aarch64-apple-darwin\.tar\.gz$' SHA256SUMS | shasum -a 256 -c
else
  sha256sum --ignore-missing -c SHA256SUMS
fi
```

Install the verified archive from a terminal. On macOS, use `tar -xzf` below rather
than Finder to avoid quarantine. If needed, remove the downloaded binary’s quarantine
with `xattr -d com.apple.quarantine tmux-sidecar` before installing:

```bash
mkdir -p "$HOME/.local/bin"
tar -xzf tmux-sidecar-v*-*.tar.gz tmux-sidecar LICENSE README.md
install -m 755 tmux-sidecar "$HOME/.local/bin/tmux-sidecar"
export PATH="$HOME/.local/bin:$PATH"
```

To build from source, install Rust ≥ 1.98 and tmux, then run from a clone. On macOS, use `rustup` to install Rust; Homebrew’s `rust` may lag the required version:

```bash
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/tmux-sidecar "$HOME/.local/bin/tmux-sidecar"
export PATH="$HOME/.local/bin:$PATH"
```

## Quick start

Run inside your tmux server. Review the hook edits, install them, and open the UI:

```bash
tmux-sidecar hooks install --dry-run
tmux-sidecar hooks install
tmux-sidecar ui
```

With no `--agent`, installation selects only agent CLIs on PATH and prints which
agents it skipped. Use `--agent` to install for an agent explicitly.

### Agent hook support

Restart agent CLIs after installing hooks. If Codex already has a notify command, use `tmux-sidecar
hooks install --agent codex --chain` to preserve and call it. Live captures: Claude
UserPromptSubmit, PreToolUse, PostToolUse, Stop, SessionEnd; Grok's same five events; Codex
`notify` plus inline UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse and
SessionEnd on Codex 0.166.0 on Linux and macOS. Codex needs these inline hooks to
report blocked; notify alone reports waiting only. On first use, Codex 0.166 shows
a "Hooks need review" screen and records hook trust in `config.toml` after review. Interrupt was proven live on Codex 0.166.0 on macOS.
Claude and Grok Notification and PermissionRequest were also proven live; StopFailure
fixtures are synthesized from documented schemas. Grok StopCancelled was proven live
on Grok 1.0.77 on macOS. OpenCode 2.2.6 failed the round-1 live check because its plugin
host was missing from the hook ancestor walk (#19). That fix has landed; a fresh live
2.2.6 check is still needed. Earlier standalone captures proved
execution.started and execution.succeeded; permission and error fixtures are synthesized.
Grok shares Claude's common entries by default; disabling compatibility uses a separate Grok file.
Exact versions and provenance: [fixture evidence](tests/fixtures/hooks/README.md).

The installer and doctor honor `CLAUDE_CONFIG_DIR` (default `~/.claude`), `CODEX_HOME`
(default `~/.codex`) and `XDG_CONFIG_HOME` (default `~/.config`, OpenCode under `opencode/plugins`).

## Doctor

```bash
tmux-sidecar doctor
tmux-sidecar doctor --json
```

Doctor reports tmux version, daemon connectivity and version, hooks for each agent, notifier
availability, config parsing, read-only history database integrity, session-board store readability,
and runtime/state directory permissions. Failed
checks include a fix hint. Exit 0 means no failures; exit 1 means something required is missing or
drifted. Uninstalled hooks, stores for agents not run yet, absent runtime/state directories
and a daemon not started yet show `skip`, excluded from the exit code. Existing unreadable
stores and insecure directories fail. `doctor --agent codex`
requires Codex hooks even without a marker; repeat `--agent` for more agents. JSON checks have
`status: "ok"`, `"fail"` or `"skip"`. Store probes follow symlinks and check only the root, without
scanning transcripts. `tmux master` is assumed current. Doctor never starts the daemon; `tmux-sidecar list` does. Version skew warns once per client process; `tmux-sidecar daemon stop` lets the
next call start the current binary. Both hooks and stores honor agent home overrides. Version skew also compares the embedded
git build id, shown by `--version` and `hello`.

Both the runtime and state directories must be owned by your uid with mode 0700.
The runtime directory holds the socket and daemon pid; the state directory holds
`tmux-sidecar.db`, `daemon.lock`, `spawn.lock`, and `daemon.socket`. The last file
records the daemon's socket so clients with different runtime directories (including
macOS clients with different `TMPDIR` values) reach the same daemon. Startup also
holds the legacy runtime `daemon.lock`; stop an older daemon before upgrading.

Set `TMUX_SIDECAR_TMUX` to an executable tmux path to override PATH for commands,
including doctor's version probe. Spawned daemons inherit the resolved executable;
an unavailable override falls back to PATH with a warning.

If a pane/process poll fails, `list` and `attention` mark rows `STALE`, and the UI
shows the health diagnostic. JSON rows expose `stale` and `stale_reason`. Historical
state stays visible, but `wait` and `send` refuse it until a successful poll.

The local socket contract is [JSON-RPC version 1](docs/rpc.md).

## MCP

Build with `cargo build --release` and put `tmux-sidecar` on your PATH. Start the
client inside the tmux server you want to observe, or add `--tmux-socket` and its
absolute socket path before `mcp` in the arguments below.

Claude Code: register the server for your user:

```bash
claude mcp add --scope user tmux-sidecar -- tmux-sidecar mcp
```

Or add it to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "tmux-sidecar": {
      "type": "stdio",
      "command": "tmux-sidecar",
      "args": ["mcp"]
    }
  }
}
```

Launch Claude Code with explicit timeouts to allow the full 600-second wait:

```bash
MCP_TOOL_TIMEOUT=610000 MCP_TIMEOUT=10000 claude
```

`MCP_TOOL_TIMEOUT` is the tool deadline and `MCP_TIMEOUT` is the startup deadline,
both in milliseconds. Set these on the Claude Code process, not in the MCP
server's `env`. The installed `claude --help` does not list them;
[Claude Code's MCP reference](https://code.claude.com/docs/en/mcp) confirms them.
The current docs describe different defaults across runtime versions, so the
example sets both explicitly.

Codex: add to `~/.codex/config.toml`:

```toml
[mcp_servers.tmux-sidecar]
command = "tmux-sidecar"
args = ["mcp"]
tool_timeout_sec = 610
```

The read tools are `list_runs` (optional `group`, `state`), `attention`,
`run_history` (`selector`, optional `limit`) and `wait_for_state` (`selector`,
`state`, optional `timeout` in seconds, default and maximum 600). Set the client's
tool timeout above your requested wait duration. A selector is a run id such as
`r42` or the current run in pane `%7`. Results contain the daemon's JSON unchanged;
RPC failures become MCP tool errors including their numeric code. A zero wait
timeout can expire before the daemon reads the run; ending before the requested
state is also a tool error.

To enable `send_prompt` (`selector`, `text`), change the arguments to
`["mcp", "--allow-send"]`. It only sends to waiting runs, with no force override. If the connection is lost
after sending, the tool reports "send outcome unknown"; check the pane before
retrying.
Without this flag the tool is absent from discovery. A waiting run finished its
turn and wants the next prompt; a blocked run needs a permission or approval.

Configuration references: [Claude Code MCP](https://code.claude.com/docs/en/mcp)
and [Codex MCP](https://developers.openai.com/codex/mcp).

MIT licensed.

MCP cancellation releases the pending daemon call; closing stdin cancels in-flight
calls and exits. rmcp 3.8 ignores syntactically malformed JSON lines instead of
replying with a parse error (official SDK behavior, rust-sdk#938). Well-formed
unknown tool calls return the protocol error `invalid_params`.

## Terminal UI

Run `tmux-sidecar ui` inside tmux. The attention queue appears first, followed by
agent runs grouped by project. Use `j/k` or arrows to move, `Enter` to switch to the
selected pane and close the UI, `a` to acknowledge, `g` to toggle manual groups,
`/` to filter (Enter/Esc finishes editing, Backspace clears characters), and `q` to
quit. `NO_COLOR=1` keeps terminal-default colors. The UI reconnects automatically
when the daemon restarts, backing off from 0.5 to 5 seconds and showing the last connection error.
Run the UI inside an attached tmux client to switch panes with Enter.

`hooks install` prints a binding you can add to your tmux config:

```tmux
bind-key a display-popup -E -w 80% -h 80% tmux-sidecar ui
```

The daemon sends desktop notifications on transitions into blocked or error using
`notify-send` on Linux or `osascript` on macOS (listed under **Script Editor** in
macOS Notification settings). Each blocked or error episode notifies once. Delivery
rechecks the current state and drops episodes that have already ended. Bursts queue for a summary at the end of the two-second delivery window. Waiting transitions display a
tmux message to attached clients in the run's tmux session. `notify.tmux = false`
disables these messages; they have their own two-second window, separate from desktop notifications. Configure desktop
notifications in tmux-sidecar's `config.toml`:

```toml
[notify]
desktop = true
tmux = true
debounce_secs = 10 # collapse repeated tmux waiting messages
```

## Uninstall

Inside the tmux server being removed:

```bash
tmux-sidecar hooks uninstall
tmux-sidecar daemon stop
```

Repeat daemon stop for each server (`--tmux-socket PATH` before `daemon stop`).
Remove the popup binding and MCP registration from your configs, then delete the
installed binary. Remove the per-server runtime and state dirs after stopping all
daemons. Linux: `$XDG_RUNTIME_DIR/tmux-sidecar` (fallback
`/tmp/tmux-sidecar-<uid>`), `${XDG_STATE_HOME:-$HOME/.local/state}/tmux-sidecar`,
and `${XDG_CONFIG_HOME:-$HOME/.config}/tmux-sidecar`. macOS:
`$TMPDIR/tmux-sidecar-<uid>` and `$HOME/Library/Application Support/tmux-sidecar`.
If set, `TMUX_SIDECAR_RUNTIME_DIR` and `TMUX_SIDECAR_STATE_DIR` replace the default
per-server directories. State deletion permanently removes agent run history.
Hook uninstall preserves unrelated entries, restores a chained Codex notify, and removes its backups.
If the history database is deleted, the next startup creates a new one and reports that history
was lost; existing history is never pruned automatically.
