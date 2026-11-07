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

**Status:** planned, no code yet. See `docs/PLAN.md`. Linux and macOS, tmux ≥ 3.3.

## MCP

Build with `cargo build --release` and put `tmux-sidecar` on your PATH. Start the
client inside the tmux server you want to observe, or add `--tmux-socket` and its
absolute socket path before `mcp` in the arguments below.

Claude Code: add to your project's `.mcp.json`:

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
RPC failures become MCP tool errors including their numeric code.

To enable `send_prompt` (`selector`, `text`), change the arguments to
`["mcp", "--allow-send"]`. It only sends to waiting runs, with no force override.
Without this flag the tool is absent from discovery. A waiting run finished its
turn and wants the next prompt; a blocked run needs a permission or approval.

Configuration references: [Claude Code MCP](https://code.claude.com/docs/en/mcp)
and [Codex MCP](https://developers.openai.com/codex/mcp).

MIT licensed.
