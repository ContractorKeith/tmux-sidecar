# tmux-sidecar

An agent-aware sidecar for tmux: one Rust binary, `tmux-sidecar`, whose per-tmux-server daemon
collects hook signals from Claude Code, Codex, Grok and OpenCode, tags them with their pane,
falls back to session-board's transcript readers, and tracks every agent run as working,
waiting, blocked, idle, error or done. It serves an attention queue, a ratatui `ui`, desktop
notifications, a guarded `send`, an MCP server and a JSON-RPC socket for Ködade. tmux owns the
terminals; tmux-sidecar owns the agent semantics. MIT. "Done" for v1 is every v1 row in
`docs/PLAN.md` §3 working on Linux and macOS and `scripts/accept.sh` passing on both.

## Status

M3 implemented (2026-11-04): pure state transitions, transactional history, transcript
fallback, attention/read marks, computed project and persistent manual groups, subscriptions.
M4 CLI verbs are next; only attention's minimal CLI is included here.
Milestones M0–M7 are GitHub issues #1–#8.

## Commands

```bash
scripts/accept.sh                                  # the acceptance: worker, reviewer and CI run this
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked                                # unit + fakes; no tmux needed
TMUX_SIDECAR_E2E=1 cargo test --locked --test e2e  # real tmux on an isolated -L socket
cargo build --release                              # target/release/tmux-sidecar
```
(Keep this block in step with `Cargo.toml`, `scripts/accept.sh` and CI.)

## Pointers

- `docs/PLAN.md` — goal, user stories, feature matrix, architecture, data model, CLI and
  socket contract, milestones, testing, acceptance.
- `CONTEXT.md` — glossary. Use its words in code, issues and PRs ("agent run", never "session").
- `docs/adr/` — daemon behind a socket; hooks, not screen scraping.
- `docs/rpc.md` — the JSON-RPC contract (from M1).
- Session logs: `~/Projects/worklog/projects/tmux-sidecar.md` (not in this repo).

## Conventions & Gotchas

- `src/engine/` is pure (signals + pane snapshots + clock in, state out). Only `store.rs`
  touches SQLite, only `tmux.rs` runs tmux, only `proc.rs` reads processes.
- Every client goes through the socket. Nothing but the daemon reads the DB.
- `tmux-sidecar hook` always exits 0, prints nothing, and returns within 200 ms; it spools when
  the daemon is down. Never make it wait.
- rusqlite stays on session-board's version (0.38, bundled); two `libsqlite3-sys` cannot link.
- Tests never touch the real `~/.claude`, `~/.codex`, `~/.grok`, `~/.config/opencode` or the
  default tmux server: set `HOME`, `TMUX_SIDECAR_RUNTIME_DIR`, `TMUX_SIDECAR_STATE_DIR` and use
  `tmux -L tsx-<random>`. Assert fake agents are first on PATH before each e2e run.
- Every test that starts a process (daemon, tmux, fake agent) kills it in a drop guard.
- `/tmp` on Omarchy is a small tmpfs; set `TMPDIR` to a scratch dir outside any checkout if it fills.
- `TMUX_SIDECAR_POLL_MS` overrides config `poll_ms` (default 1000); tests use 100.
- Every e2e body has a 60 s watchdog with scratch-tree/log diagnostics; CI jobs time out at 20 min.
- `daemon.log` is capped at 1 MiB and rotated once to `daemon.log.1`.
- A detached daemon needs its own lifetime lock as well as `spawn.lock`: foreground starts
  must never unlink a live socket. Keep lock descriptors CLOEXEC.
- Hook payload format is independent of ancestor identity: Grok uses Claude entries by default.
- Appenders never lock: one O_APPEND frame write plus an inode check/retry covers replay rename
  races. Replay owns `spool.replaying.*` claims and recovers leftovers before the live spool.
- Both hook ancestor walks avoid subprocesses: `/proc` on Linux, `proc_pidinfo` on macOS.
- Spool before live delivery; receipts dedupe invocation nonces, not equal payloads. Reject bad lines without stopping the daemon.
- Cache server_start in the runtime dir; keep hook ancestry/probes cheap and drain stdin before no-op paths.
- OpenCode 2.2.5 uses a default id/setup plugin, V2 data fields, and the opencode.exe process name.
- Codex inline hook edits use toml_edit; --chain preserves old notify argv after recording.
- Transcript readers and per-request project resolution run on blocking tasks, never the SQLite
  owner or RPC task. session-board ids carry an agent prefix; compare native hook ids after stripping it.
- Focus read marks require pane_active + window_active + session_attached; covered with two
  control-mode attached clients, a hidden window and a detached tmux session.
- Linux process exit probes treat both ENOENT and ESRCH from `/proc/<pid>/stat` as exited;
  the daemon may disappear between signal-0 and the stat read.
- Subscribers have 256-change queues and bounded writes; state_changed history commits with state.
- Commits: `<type>: <description>`, author @ContractorKeith only.

## Tracking

- GitHub issues are the tickets, one per milestone, label `milestone`; `skeptic` for review findings.
- Basecamp project "github" card `tmux-sidecar`, one step per milestone (`project-board` skill).
- Per PR: worker's `scripts/accept.sh` → orchestrator full-diff read and rerun → `pr-stress`
  when it applies → fixes → one PR comment → `gh pr ready` → merge. `skeptic-review` on
  Omarchy and the Mac Studio is the release gate before a tag.

## Out of Scope

- Own PTY/VT engine, Zellij support, history pruning. Never.
- Approve/deny automation, screen-scraping rules, remote/TCP access, multi-server `ui`,
  launching agents. Later, if real use asks.
