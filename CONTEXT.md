# tmux-sidecar

An agent-aware sidecar for tmux: it knows what each coding agent running in a tmux pane is
doing, and lets people and tools see and drive those agents without attaching.

## Language

**Pane**:
A tmux pane, identified by its tmux pane id (`%12`). The unit tmux-sidecar tracks.
_Avoid_: Window, terminal, tab

**Agent**:
A coding-agent CLI running in a pane: Claude Code, Codex, Grok or OpenCode.
_Avoid_: Bot, assistant, worker

**Agent state**:
What an agent in a pane is doing right now: working, waiting, blocked, idle, error or done.
_Avoid_: Status (sboard's word for its five-value enum), activity

**Waiting**:
The agent finished its turn and wants the next prompt.
_Avoid_: Idle, ready

**Blocked**:
The agent is mid-turn and stopped on a permission or approval request.
_Avoid_: Waiting, paused, stuck

**Signal**:
One piece of evidence about agent state: a hook event or a transcript reading.
Hooks outrank transcripts.
_Avoid_: Event (reserved for hook payloads), update

**Daemon**:
The one background tmux-sidecar process that owns agent state and serves the socket.
_Avoid_: Server (tmux's word for its own process), service, backend

**Client**:
Anything that talks to the daemon over its socket: the hook command, CLI, sidebar or MCP server.
_Avoid_: Frontend, consumer

**Agent run**:
One agent process in one pane, from start to exit; the key for state and history. Carries the agent's own session id once known.
_Avoid_: Session (ambiguous between tmux session and agent session), job

**tmux session**:
tmux's own session; always written in full, never bare "session".
_Avoid_: Session, workspace

**Project group**:
The automatic group of every pane whose cwd shares a git root.
_Avoid_: Workspace, repo group

**Manual group**:
A named set of panes a person chose; a pane can be in many.
_Avoid_: Tag, label, folder

**Attention queue**:
The ordered list of agent runs that need a person: blocked, then error, then waiting, oldest first, minus read ones.
_Avoid_: Inbox, notifications, alerts

**Read**:
An attention item the person has seen (pane focused or acked); a new state change clears it.
_Avoid_: Dismissed, done
