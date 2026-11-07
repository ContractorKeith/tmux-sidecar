# tmux-sidecar — Build Plan

Status: M0 in progress (2026-11-04). Sources studied: Herdr, TuiOS, Zellij, Muxy, and
Keith's own session-board (sboard). Decisions come from the 2026-11-04 grill; terms are in
`CONTEXT.md`, and the hard-to-reverse choices are in `docs/adr/`.

## 1. Goal

`tmux-sidecar` is one Rust binary that sits beside tmux and knows what each coding agent in
each pane is doing: working, waiting for the next prompt, blocked on an approval, idle, errored,
or done. It shows that in a status UI and an attention queue, raises a desktop notification when
an agent is blocked, types prompts into a waiting agent without attaching, and lets other
programs (Ködade, any MCP client, scripts) read and drive the same state over one local API.
tmux keeps owning the terminals; tmux-sidecar owns the agent semantics.

v1 is done when every row marked **v1** in §3 works on Linux and macOS with tmux ≥ 3.3, and
`scripts/accept.sh` passes from a clean clone on both platforms.

## 2. Problem, solution, user stories

**Problem.** Running four or more agent CLIs in tmux, the person has to cycle through panes to
find the one that is stuck on a permission prompt or finished its turn. tmux has no idea what an
agent is. Agents already report their state through hooks, but nothing collects it per pane,
and nothing lets another tool ask "which agents need me?" or "tell the Codex in %7 to run the
tests" safely.

**Solution.** Agent hooks call `tmux-sidecar hook`, which tags each signal with its tmux pane
and hands it to a per-tmux-server daemon. The daemon tracks every agent run, falls back to
sboard's transcript reading where hooks are silent, persists everything to SQLite, and serves
a JSON-RPC socket that the CLI, the `ui`, the MCP server and Ködade all use.

User stories:

1. As a person running several agents, I want one pane that lists every agent run with its
   state, so I can see at a glance who needs me.
2. As that person, I want blocked runs to come first and to raise a desktop notification, so an
   approval prompt never sits unseen for twenty minutes.
3. As that person, I want focusing a pane in tmux to mark its attention item read, so the
   queue clears itself as I work.
4. As that person, I want `tmux-sidecar send %7 "run the tests"` to type into a waiting agent
   and to refuse while it is working, so I never corrupt an agent mid-turn.
5. As a script author, I want `tmux-sidecar wait %7 --until waiting --timeout 600` to block
   until the run reaches that state, so I can chain agent work.
6. As an agent (via MCP), I want to list other agents and read their history, and — only when
   the person started the server with `--allow-send` — prompt a waiting one.
7. As Ködade, I want a documented, versioned JSON-RPC contract with pushed state changes, so I
   can draw the same state without polling.
8. As a person, I want group views by project automatically and by my own named groups, so a
   dozen panes across tmux sessions stay readable.
9. As a person, I want `hooks install --dry-run` to show exactly what it would change in my
   agent configs and `hooks uninstall` to remove only its own entries.
10. As a person, I want the agent hooks never to slow or fail an agent turn, even when the
    daemon is down, and the missed signals to show up once it is back.
11. As a person, I want `doctor` to tell me what is missing: tmux version, hooks not installed
    or drifted, daemon not reachable, sboard stores unreadable.
12. As a person with two tmux servers (`-L work`), I want each to have its own daemon, state
    and history, never mixed.

## 3. Feature matrix

**v1** = in scope now. **Later** = after v1. **Drop** = deliberately never.

| # | Feature | Decision | M | Notes |
|---|---|---|---|---|
| 1 | Crate layout, seams (tmux, process tree, clock), CI on Linux + macOS, `accept.sh` | v1 | M0 | |
| 2 | Daemon auto-spawn, per-tmux-server socket, JSON-RPC 2.0 NDJSON | v1 | M1 | ADR-0001 |
| 3 | Pane discovery by polling `tmux list-panes -a` (1 s) | v1 | M1 | no control mode, no tmux set-hook |
| 4 | Agent run detection from the pane process tree | v1 | M1 | |
| 5 | SQLite store: runs, signals, groups, names, read marks; kept forever | v1 | M1 | |
| 6 | `tmux-sidecar hook` command, spool when the daemon is down, 200 ms budget | v1 | M2 | |
| 7 | `hooks install/uninstall [--dry-run]` for Claude Code, Codex, Grok, OpenCode | v1 | M2 | marked entries only |
| 8 | Recorded hook payload fixtures for all four agents | v1 | M2 | captured by hand once |
| 9 | State engine: six agent states, hook mapping, process exit → done | v1 | M3 | ADR-0002 |
| 10 | sboard transcript fallback for runs with no hook signal | v1 | M3 | git dep on session-board v0.1.0 |
| 11 | Attention queue (blocked > error > waiting, oldest first) and read marks | v1 | M3 | focus marks read |
| 12 | Project groups (git root) and manual groups (many-to-many) | v1 | M3 | |
| 13 | CLI verbs (§7), `--json` on every read verb, `subscribe`-backed `watch` | v1 | M4 | |
| 14 | `send` with waiting-only guard and bracketed paste | v1 | M4 | `--force` overrides |
| 15 | `ui`: ratatui client, popup keybind | v1 | M5 | |
| 16 | Desktop notifications on blocked/error; tmux `display-message` on waiting | v1 | M5 | |
| 17 | `mcp` stdio server (rmcp): reads; `send` only with `--allow-send` | v1 | M6 | |
| 18 | `doctor`, release workflow (Linux x86_64/aarch64, macOS arm64), install docs | v1 | M7 | |
| 19 | Approve/deny automation for blocked runs | Later | | per-agent keys; wrong key approves the wrong call |
| 20 | Screen-scraping state rules | Later | | ADR-0002; only if an agent loses its hooks |
| 21 | Several tmux servers in one `ui` | Later | | one daemon per server stays |
| 22 | Remote access (SSH socket forwarding docs, TCP) | Later | | local only in v1 |
| 23 | Launching agents / worktrees from tmux-sidecar | Later | | sboard already does this |
| 24 | Own PTY/VT engine, replacing tmux | Drop | | Herdr, TuiOS and Muxy own that space |
| 25 | Zellij support | Drop | | different plugin model |
| 26 | History pruning | Drop | | Keith keeps history forever |

## 4. Inspirations and what to skip

| Source | Copy | Skip |
|---|---|---|
| Herdr (Apache-2.0, Rust) | JSON socket with published schema; `agent wait`; `pane send` | own VT engine, TOML screen rules, remote rule updates |
| TuiOS (MIT, Go) | signal order: hooks first; `tuios mcp` | being a full window manager |
| Muxy (MIT, Rust) | `session wait`, `read-screen`; `--json` everywhere | GPUI desktop app, phone apps, CBOR |
| Zellij | — | WASM plugin route (ADR context) |
| session-board | transcript readers, `status::derive`, tmux pane parsing, ratatui style | its "never owns" scope; its launch features |

## 5. Architecture

```
src/
  main.rs          clap entry; dispatches verbs
  lib.rs
  model.rs         AgentRun, AgentState, Signal, Group, ids
  seams.rs         traits Tmux, ProcessTree, Clock + in-memory fakes (test feature)
  paths.rs         per-server key, runtime dir, state dir, config path
  tmux.rs          real Tmux: list-panes, display-message, load-buffer/paste-buffer, send-keys
  proc.rs          real ProcessTree via `ps -A -o pid=,ppid=,comm=` (Linux and macOS)
  store.rs         SQLite schema and queries (only module that touches the DB)
  engine/          pure state engine: signal → state, transcript fallback, attention, groups
  daemon/          tokio: socket server, poll loop, spool replay, subscriptions, notify
  rpc.rs           JSON-RPC types, method table, error codes, schema version
  client.rs        socket client with auto-spawn (used by every non-daemon verb)
  hook.rs          `hook` verb: parse payload, find agent ancestor, send or spool
  install/         hooks install/uninstall per agent (claude.rs, codex.rs, grok.rs, opencode.rs)
  cli/             verb handlers and human/JSON output
  ui/              ratatui client
  mcp.rs           rmcp stdio server over client.rs
  notify.rs        notify-send / osascript, debounced
docs/
  PLAN.md  adr/  rpc.md (the JSON-RPC contract, versioned)
tests/
  e2e.rs           real tmux on an isolated -L socket, fake agents
  fixtures/hooks/  recorded payloads per agent and event
scripts/accept.sh
```

Rules that hold the shape:
- `engine/` is pure: it takes signals, pane snapshots and the clock, returns state. No I/O.
  Everything hard is tested there on fakes.
- Only `store.rs` touches SQLite; only `tmux.rs` runs `tmux`; only `proc.rs` reads processes.
- Every client — CLI, `ui`, `mcp`, hooks, Ködade — goes through the JSON-RPC socket. No client
  reads the DB directly.
- The daemon is the only tmux poller. Hooks never wait on the daemon.
- The agent's own transcripts are read-only input, through sboard's lib only.

### Dependencies and why

| Crate | Why |
|---|---|
| tokio | daemon: socket server, many concurrent subscriptions, poll and debounce timers |
| serde, serde_json | JSON-RPC and hook payloads |
| clap (derive) | CLI |
| rusqlite 0.38 (bundled) | store. Must match session-board's version: two `libsqlite3-sys` versions cannot link. |
| ratatui 0.30, crossterm 0.29 | `ui`; same as sboard |
| rmcp | official Rust MCP SDK for the stdio server; hand-rolling MCP framing costs more |
| session-board (git, tag v0.1.0) | transcript readers and status derivation for all four agents |
| toml | config and Codex `config.toml` editing (`toml_edit` if comments must survive) |
| libc | peer uid check on the socket, `getppid` chain |
| tempfile (dev) | isolated homes and state dirs in tests |

Nothing else without a line in this table.

## 6. Data model

**Server key.** The tmux socket path (from `$TMUX`, first comma field, or
`tmux display -p '#{socket_path}'`), hashed to 12 hex chars. Every path below is per key.
- Runtime dir: `$XDG_RUNTIME_DIR/tmux-sidecar/<key>/` (Linux), `$TMPDIR/tmux-sidecar-<uid>/<key>/`
  (macOS). Mode 0700. Holds `sock` (0600) and `daemon.pid`.
- State dir: `$XDG_STATE_HOME/tmux-sidecar/<key>/` (default `~/.local/state`), macOS
  `~/Library/Application Support/tmux-sidecar/<key>/`. Holds `tmux-sidecar.db` and `spool.ndjson`.
- Config: `$XDG_CONFIG_HOME/tmux-sidecar/config.toml` (macOS `~/Library/Application Support/tmux-sidecar/config.toml`).
- `TMUX_SIDECAR_RUNTIME_DIR` and `TMUX_SIDECAR_STATE_DIR` override both (tests use them).

**Pane key.** `(server_start, pane_id)` where `server_start` is `#{start_time}` of the tmux
server and `pane_id` is `%N`. Names and manual group membership attach to the pane key, so
they survive an agent restarting in the same pane but not a tmux server restart.

**Agent run.** One agent process in one pane. Key `(server_start, pane_id, agent_pid)`; a
short public id `r<rowid>`. Selectors accepted everywhere: `r42`, or `%7` meaning the current
(newest, not ended) run in pane %7 of this server.

```
runs(id, server_start, pane_id, agent, agent_pid, agent_session_id NULL, cwd, project_root,
     started_at, ended_at NULL, state, state_since, state_source, read_at NULL,
     last_assistant_text, transcript_path NULL)
signals(id, run_id, at, source, kind, payload_json)      -- every signal, forever
panes(server_start, pane_id, name NULL)                    -- user names
groups(id, name UNIQUE); group_members(group_id, server_start, pane_id)
meta(schema_version)
```

**Agent state.** `working | waiting | blocked | idle | error | done`.

| Signal | → state |
|---|---|
| Claude/Grok `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | working |
| Claude/Grok `Notification` `permission_prompt`, Claude/Grok `PermissionRequest` | blocked |
| Claude/Grok `Stop`, `Notification` `idle_prompt` | waiting |
| Grok `StopCancelled` | waiting |
| Claude/Grok `StopFailure` | error |
| Claude/Grok/Codex `SessionEnd` | done |
| Codex `notify` `agent-turn-complete`; Codex `Interrupt` | waiting |
| Codex `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | working |
| Codex `PermissionRequest` | blocked |
| OpenCode `session.status` busy | working |
| OpenCode `permission.asked` | blocked |
| OpenCode `permission.replied` | working |
| OpenCode `session.idle` | waiting |
| OpenCode `session.error` | error |
| agent pid gone from the pane | done |
| waiting for longer than `idle_after` (default 30 min) and read | idle |

Precedence: once a run has received any hook signal, transcripts never set its state (they
still fill `last_assistant_text` and `transcript_path`). A run with no hook signal takes its
state from sboard's derivation, mapped one to one (sboard has no blocked). The OpenCode event
names above were confirmed against the installed OpenCode 2.2.5 binary on 2026-11-04.

**Attention queue.** Runs not ended, state in {blocked, error, waiting}, `read_at` null or
older than `state_since`; ordered blocked, error, waiting, then `state_since` ascending.
Focusing the pane (the poll sees `#{pane_active}` in the active window of an attached client)
or `ack` sets `read_at`. Any state change makes the item unread again.

**Groups.** Project group = git root of the run's cwd (sboard's `project` rules), computed,
not stored. Manual groups are stored and contain pane keys.

## 7. CLI and API contract

```
tmux-sidecar list [--group G] [--state S] [--all] [--limit N] [--json]  active runs; --all includes history (newest first, limit 100)
tmux-sidecar attention [--json]                            the attention queue
tmux-sidecar watch [--json]                                stream state changes until Ctrl-C
tmux-sidecar wait <sel> --until waiting|blocked|done [--timeout SECS] [--json]
tmux-sidecar send <sel> <text>|-  [--force]                paste + Enter; refuses unless waiting
tmux-sidecar ack <sel>...                                  mark read
tmux-sidecar history <sel> [--limit N] [--json]            signals and state changes
tmux-sidecar name <sel> <name>|--clear                     name the pane
tmux-sidecar group add|rm <group> <sel>...   group ls [--json]
tmux-sidecar hooks install|uninstall [--agent A]... [--dry-run]
tmux-sidecar hook <claude|codex|codex-notify|grok|opencode> [payload]   (called by agents)
tmux-sidecar daemon status [--json] | stop | run            (`run` = foreground, for debugging)
tmux-sidecar doctor [--json]
tmux-sidecar ui
tmux-sidecar mcp [--allow-send]
```

Global flags: `--tmux-socket PATH` (else `$TMUX`, else tmux's default socket), `-q`.
Config precedence: flag > env > config file > default. Config keys: `poll_ms` (1000),
`idle_after_secs` (1800), `notify.desktop` (true), `notify.debounce_secs` (10).

Exit codes: 0 ok · 1 error · 2 usage · 3 refused (send guard) · 4 timeout (wait) ·
5 not found (selector) · 6 daemon unreachable after auto-spawn.

`hook` always exits 0 and prints nothing, whatever happens; failures go to
`<state dir>/hook.log` (capped at 1 MiB, rotated once).

**Socket.** Newline-delimited JSON-RPC 2.0. Methods: `hello` (returns `rpc_version: 1`,
daemon version, server key), `list`, `attention`, `history`, `wait`, `send`, `ack`, `name`,
`group.add`, `group.rm`, `group.ls`, `subscribe` (server pushes `state_changed` notifications
on the same connection), `signal` (from hooks), `daemon.status`, `daemon.stop`. Error codes
mirror exit codes (`-32003` refused, `-32004` timeout, `-32005` not found). `docs/rpc.md` is the
contract; a test checks every method in the table is documented and every documented method
exists.

**Auto-spawn.** A client that cannot connect takes an exclusive lock on
`<runtime>/spawn.lock`, re-checks, starts `tmux-sidecar daemon run` detached (setsid, stdio to
`<state>/daemon.log`), and waits up to 2 s for the socket. The hook command never waits: it
spools, triggers a detached spawn, and exits.

**Hook delivery.** `hook` reads the payload (stdin; Codex `notify` passes JSON as the last
argv), reads `$TMUX` and `$TMUX_PANE` (no `$TMUX` → exit 0, nothing recorded), walks its own
ancestor chain to the first process named `claude`, `codex`, `grok` or `opencode` (that is the
agent and its pid; the argument only says which payload format to parse), then sends one
`signal` notification with a 100 ms connect/write timeout. On failure it appends the same line
to `spool.ndjson` under `flock`. Total budget 200 ms. The daemon replays and truncates the spool
at start and whenever it sees the file grow.

**Hooks install.** Claude Code: entries in `~/.claude/settings.json` for Notification,
PermissionRequest, UserPromptSubmit, PreToolUse, PostToolUse, Stop, StopFailure, SessionEnd,
each command `tmux-sidecar hook claude` plus a marker (`"_tmux_sidecar": 1` in the entry, or a
trailing `# tmux-sidecar` comment where the format has no extra fields). Grok reads that file
by default. Grok-only StopCancelled always lives in `~/.grok/hooks/tmux-sidecar.json`.
If `claude_hooks_enabled = false`, common Grok events instead live in the separate
`~/.grok/hooks/tmux-sidecar-common.json`, so the Grok-only file never duplicates them. Codex: `notify` in
`~/.codex/config.toml` (refuse and explain if the user already has a different `notify`
program; offer `--chain`), plus UserPromptSubmit, PreToolUse, PostToolUse, PermissionRequest, Interrupt and SessionEnd hooks. OpenCode: a plugin file
`~/.config/opencode/plugin/tmux-sidecar.js` that forwards the five events by spawning
`tmux-sidecar hook opencode`. `--dry-run` prints a unified diff per file and changes nothing.
Every write is atomic (temp file + rename) and keeps a `.tmux-sidecar.bak` of the first
original.

## 8. Milestones

Each milestone is one GitHub issue, one branch, one draft PR, one worker thread in its own
worktree.

| M | Deliverable | Depends on | Done when |
|---|---|---|---|
| M0 | Crate, module skeleton, seams + fakes, `paths.rs`, `rpc.rs` types, CI (ubuntu + macos), `accept.sh` | – | `accept.sh` green on both runners; rows 1 |
| M1 | Daemon: auto-spawn, socket + peer uid, JSON-RPC `hello/list/daemon.*`, poll loop, run detection, SQLite store | M0 | e2e: fake agent in real tmux appears in `list --json`, pane close ends it; rows 2–5 |
| M2 | `hook` command, spool + replay, `hooks install/uninstall/--dry-run` ×4, payload fixtures | M1 | e2e: fixtures replayed by fake agents land as signals; daemon down → spool → replay; install round-trips byte-identical; rows 6–8 |
| M3 | State engine, sboard fallback, attention, read marks, groups | M2 | engine table tests for every row of §6; e2e blocked → waiting → done; rows 9–12 |
| M4 | CLI verbs, `watch`, `wait`, `send` guard and paste | M3 | e2e for every verb and exit code in §7; rows 13–14 |
| M5 | `ui` and notifications | M3 | ui renders fixtures (ratatui TestBackend snapshots); notify debounced; rows 15–16 |
| M6 | MCP server | M3 | stdio session lists runs; `send` absent without `--allow-send`; rows 17 |
| M7 | `doctor`, release workflow, README install, `docs/rpc.md` finalised | M4, M5, M6 | release dry run builds three targets; rows 18 |

M4, M5 and M6 run in parallel after M3. Merge order M4, M6, M5 (M5 last because the UI is
the likeliest to need rebasing onto final RPC shapes).

## 9. Testing

- Seams: `Tmux`, `ProcessTree`, `Clock`. The engine, attention, guards and install editors are
  tested on fakes and fixtures; adapters are covered by e2e.
- e2e (`tests/e2e.rs`, `TMUX_SIDECAR_E2E=1`): real tmux on `-L tsx-<random>`, isolated
  runtime/state/config dirs, and a fake agent — a small test binary copied (not symlinked) as
  `claude`, `codex`, `grok`, `opencode` into a temp `bin/` placed first on PATH, which the
  test asserts. The fake runs `tmux-sidecar hook <agent>` with a fixture payload on command,
  so its ancestor chain is real.
- Fixtures: `tests/fixtures/hooks/<agent>/<event>.json`, captured once from the real CLIs by a
  capture hook in M2 and committed with secrets and paths scrubbed.
- Real agent CLIs never run in CI. Every test that starts a process kills it; e2e kills its
  tmux server in a drop guard.

## 10. Acceptance

`scripts/accept.sh` is the acceptance: fmt, clippy `-D warnings`, `cargo test --locked`,
release build, e2e against real tmux, then live checks with the release binary. Worker,
reviewer and CI run the same script; each milestone adds its own live checks. v1 acceptance
also includes a manual run on Omarchy and the Mac Studio with real Claude Code, Codex, Grok and
OpenCode sessions (recorded in the skeptic report): each agent shows blocked on a real
permission prompt and waiting after its turn.

## 11. Process

- One milestone per worker thread (Codex gpt-6.1-sol, medium) briefed with `worker-brief`;
  draft PR; orchestrator reads the full diff and reruns `scripts/accept.sh`; `pr-stress` by a
  fresh thread when the diff touches its table; fixes in the author's thread; one PR comment
  per round; `gh pr ready`; merge.
- Before the tag: `skeptic-review` on main from fresh Opus 5.5 sessions on Omarchy and the
  Mac Studio; one labelled issue per confirmed bug.
- Release through `ship`. GitHub issues are the truth; the Basecamp card has one step per
  milestone. The orchestrator writes the worklog after each merge; workers report only.

## 12. Open questions

- M3 retains focus read marks: `pane_active` + `window_active` + `session_attached` identifies
  visible panes. The real-tmux test uses two attached control-mode clients, a hidden window
  and a detached tmux session; only visible panes are read, and switching windows reads the
  newly visible pane. This measures tmux visibility, not operating-system window focus.
- Codex `notify` collides with an existing user `notify` program; `--chain` semantics are
  settled in M2.
- session-board v0.1.0's lib API was not designed as a public interface. If M3 needs
  something it doesn't expose, pin a later commit; extract a shared crate only if that
  happens twice.
