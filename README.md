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

MIT licensed.
