---
name: skeptic-env
description: "How a skeptical reviewer builds, runs and attacks tmux-sidecar. Read by the skeptic-review and pr-stress skills; not a workflow on its own."
---

# skeptic-env — tmux-sidecar

One Rust binary, `tmux-sidecar`. A daemon per tmux server collects agent hook signals tagged
with their pane, falls back to session-board's transcript readers, and tracks agent runs as
working / waiting / blocked / idle / error / done. Clients (CLI, `ui`, `mcp`, hooks) talk to
it over a JSON-RPC Unix socket. "Done" for v1 is every v1 row in `docs/PLAN.md` §3 working on
Linux and macOS and `scripts/accept.sh` passing on both.

## Stack and commands

```bash
scripts/accept.sh                      # the whole gate, exactly as CI
cargo build --release && ./target/release/tmux-sidecar --version
TMUX_SIDECAR_E2E=1 cargo test --locked --test e2e
```

Scratch state for any manual run: a private tmux server (`tmux -L tsx-skeptic`) and
`HOME`, `TMUX_SIDECAR_RUNTIME_DIR`, `TMUX_SIDECAR_STATE_DIR` pointed at a temp dir, so a review
never edits the real `~/.claude/settings.json`, `~/.codex/config.toml`, `~/.grok` or
`~/.config/opencode`, and never attaches a daemon to Keith's working tmux server. The live
real-agent check (PLAN §10) is the one exception, and it runs `hooks install` against a
scratch `HOME` with the real agent binaries.

## Platforms

| Platform | Ships | Review on | Notes |
|---|---|---|---|
| Linux | yes | Omarchy | real Claude Code, Codex, Grok, OpenCode installed; notify-send |
| macOS | yes | Mac Studio | real Claude Code and Codex; osascript notifier; `$TMPDIR` runtime dir |
| Windows | no | | |

## What to attack

- **Hooks**: daemon down, daemon mid-restart, two hooks in the same millisecond, a payload
  that is not JSON, a 10 MB payload, a hook run outside tmux, a hook whose ancestor chain has
  no agent (a user running it by hand), Grok firing the Claude entry, the 200 ms budget under
  load, spool replay after a crash mid-append.
- **Install**: settings files with comments, trailing commas, CRLF, a symlinked settings file,
  an existing Codex `notify`, running install twice, uninstall after the user edited an entry,
  `--dry-run` really writing nothing.
- **Daemon**: two clients spawning at once, a stale socket after `kill -9`, a socket dir with
  wrong permissions, another user's socket, tmux server restart (pane ids reused),
  `-L` servers kept apart, upgrade with the old daemon still running.
- **State**: every row of PLAN §6 live; a hook and a transcript disagreeing; an agent that
  exits while blocked; idle timing; read marks when two clients are attached.
- **send**: a working run, a run that turns working between check and paste, multi-line
  text, text with tmux key names (`C-c`, `Enter`), a pane that was closed.
- **MCP**: `send` visible without `--allow-send`, stdin EOF mid-request.
- **Test honesty**: an acceptance claim with no test; a test that still passes with its
  assertion removed; e2e skipped silently on one runner.

## Traps

- If /tmp is full on Omarchy, set TMPDIR to a scratch directory outside every git checkout.
- Kill every daemon, tmux server and fake agent you start; orphans pile up across rounds.
- Assert fake agents are first on PATH before trusting an e2e result.
