# One daemon per tmux server, behind a Unix socket

Agent state lives in one background process per tmux server, and every client (agent hooks,
CLI, `ui`, MCP server, Ködade) talks to it over a newline-delimited JSON-RPC Unix socket. We
chose this over a daemonless design (hooks append to per-pane files; every reader watches the
files) because pushed subscriptions, a single tmux poller, a single SQLite writer and one
versioned contract for Ködade are simpler to get right with one owner than with N readers
racing on files. The cost is lifecycle: auto-spawn with a lock, stale sockets, upgrades
mid-session, and hooks that must never wait on a daemon that is down (they spool instead).
Reversing it later means rewriting every client's transport, so the socket contract is
versioned from day one.
