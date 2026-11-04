# tmux-sidecar

An agent-aware sidecar for tmux: one Rust binary, `tmux-sidecar`, whose per-tmux-server daemon
collects hook signals from Claude Code, Codex, Grok and OpenCode, tags them with their pane,
falls back to session-board's transcript readers, and tracks every agent run as working,
waiting, blocked, idle, error or done. It serves an attention queue, a ratatui `ui`, desktop
notifications, a guarded `send`, an MCP server and a JSON-RPC socket for Ködade. tmux owns the
terminals; tmux-sidecar owns the agent semantics. MIT. "Done" for v1 is every v1 row in
`docs/PLAN.md` §3 working on Linux and macOS and `scripts/accept.sh` passing on both.

## Status

M1 implemented (2026-11-04): per-server daemon, secure socket, auto-spawn, process polling,
WAL store, list/status/stop and RPC contract. M2 hooks and M3 state semantics are next.
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
- `daemon.log` is capped at 1 MiB and rotated once to `daemon.log.1`.
- A detached daemon needs its own lifetime lock as well as `spawn.lock`: foreground starts
  must never unlink a live socket. Keep lock descriptors CLOEXEC.
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
