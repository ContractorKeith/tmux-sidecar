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

**Status:** M0–M6 implemented; release work continues. See `docs/PLAN.md`. Linux and macOS, tmux ≥ 3.3.

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
`notify-send` on Linux or `osascript` on macOS. It limits delivery to once per run
per 10 seconds and globally once every 2 seconds. Waiting transitions display a
tmux message to attached clients in the run's tmux session. `notify.tmux = false`
disables these messages; they share the global two-second debounce with desktop notifications. Configure desktop
notifications in tmux-sidecar's `config.toml`:

```toml
[notify]
desktop = true
tmux = true
debounce_secs = 10
```
