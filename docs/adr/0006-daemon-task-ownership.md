# Daemon task ownership and bounded I/O

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003). The rule behind
all of these: the SQLite owner and the RPC dispatcher never wait on slow work, and every
socket or subprocess operation has a bound.

## Who owns what

- `src/daemon/store_worker.rs`: one blocking owner keeps SQLite open and runs queries,
  transactions and close off the tokio threads. Only sends hold run locks; `with` acquires them
  in stable order, and the mutation gate stops a send registering a new lock until the job ends.
- `src/daemon/state_methods.rs`: SQLite-only RPC operations. Project resolution is prepared
  outside this module, on blocking tasks (`src/daemon/list.rs` pages stored candidates first).
- `src/daemon/actions.rs`: long-lived operations (wait, send) run independently of the RPC
  dispatcher and the SQLite owner.
- `src/daemon/poll.rs` `tick`: external commands can be slow, so pane/process collection runs on
  a blocking task, never on the SQLite owner.
- `src/daemon/replay.rs` `replay`: replay failures are logged and never charged to tmux/process
  poll health.
- `src/notify.rs`, `src/notify/backend.rs`: desktop notifications consume committed changes on
  their own task; socket lookups and notification commands run only there. `deliver` sends the
  summary to every affected tmux session, not just the first pane's.
- `src/store/view.rs`: the UI digest streams relevant columns into a hasher; it never allocates
  or deep-compares JSON.

## Wait and subscribe

- `src/daemon/actions.rs` `wait`: subscribe before reading, so a change between the snapshot and
  `recv` cannot be lost. Match on each committed transition, including transient states already
  followed by another.
- `src/ui/stream.rs`: the UI subscribes before loading, so changes during the snapshot stay
  queued on the socket.

## Send

- `src/daemon/send.rs` `send`: slow probes run before the run lock; cancellation kills them
  before any paste can start. Terminal work is always joined before replying, and its absolute
  deadline is checked before every tmux command, so a delayed blocking task cannot paste after
  timeout.
- `src/seams.rs` `validate_prompt`: prompt text cannot escape bracketed paste or synthesize
  terminal keys.
- `src/tmux/paste.rs`: named buffers keep prompts out of tmux's key parser and other clients'
  buffers.
- `src/tmux/actions.rs`: tmux mutations are bounded and always target the explicitly selected
  server. `display-message` expands formats even with argv, so `#` is escaped to `##` to keep
  messages literal, including future user-provided names.

## Transport

- `src/daemon/transport.rs` `serve_with_peer`: one frame is size-limited so a local client
  cannot grow the daemon without bound. Every write, including the final one, has a 250 ms bound
  so a blocked socket never holds the daemon. Ordinary RPCs own their reply before the next
  read; only a long-lived wait needs a concurrent disconnect monitor, and pipelined requests
  read during that wait are processed after its reply.
- `src/client.rs` `exchange`: a rejection may already be queued when the peer closes before
  hello's write, so the client reads it before reporting the write error. `warn_version` warns
  once per client process, including across UI reconnects and MCP calls.
- `src/cli/output.rs` `print_read`: empty queues carry no row diagnostics, so human output
  consults daemon health to explain an empty result.

## MCP

- `src/mcp.rs`: the stdio MCP tools are a thin adapter over the RPC socket and share the CLI's
  ordinary and send budgets and its outcome-unknown errors.
- `src/mcp/tools.rs` `prepare`: arguments are validated even when a client bypasses the
  advertised input schema. A tool absent from discovery (`send_prompt` without `--allow-send`)
  is also refused on direct invocation.
- `src/mcp/input.rs`: stdin EOF is signalled before rmcp begins draining outstanding handlers.
- `src/proc/bounded.rs`: deadline-bound probes kill their child on cancellation and drain both
  pipes.

## UI terminal

- `src/ui/mod.rs` `run`: signal handlers are registered before entering raw mode, so termination
  always returns through `Restore`. The first frame is drawn before input initialization and
  daemon I/O.
- `src/ui/terminal.rs`: the terminal is restored before panic diagnostics print or a signal
  returns. `set_hook` is forbidden while unwinding, and the installed hook has already chained,
  so `drop` skips it then.
