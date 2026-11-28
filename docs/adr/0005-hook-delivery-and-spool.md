# Hook delivery, spool and attribution

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003). See also the
hook bullets in `AGENTS.md`.

## Input and delivery

- `src/hook.rs`: drain hook input, then attempt a durable append before live delivery, and
  still deliver live when the append fails.
- `src/hook/input.rs`: read to EOF while input progresses. Give up only after a second without
  progress; EOF and the 64 MiB cap also end reading.
- `src/hook/delivery.rs` `record`: exactly one O_APPEND write precedes live delivery and daemon
  spawning.
- `src/hook.rs` `log_agent`: logging uses the cached pane boundary and never runs a tmux probe.
  `unknown_format` still has a log destination when invoked outside tmux.
- `src/hook/files.rs`: hook-owned files never follow links or block on a FIFO.
- `src/hook/context.rs` `resolve`: a zero `server_start` marks a durable frame captured before
  the first daemon poll; it is filled from the runtime cache at replay.

## Spool and replay

- `src/hook/spool.rs`: appenders are lock-free. One O_APPEND write per frame means frames cannot
  interleave. Replay may have read or unlinked an inode an appender still holds, so the appender
  retries the same nonce in the fresh live spool and SQLite receipts deduplicate either copy.
- `src/hook/spool.rs` `claim`: recover prior claims first, then atomically claim the live file
  by rename, and recreate an empty live file for new appenders and spool-health readers. Replay
  never rewrites the live spool.
- `src/daemon/replay.rs` `inner`: a failed claim is retained, but independent claims still make
  progress.
- `src/store/signals.rs` `signal`: receipt and signal commit together, so replay can safely
  repeat after a crash.
- `tests/support/hooks.rs`: a busy daemon may legitimately make a hook spool; tests allow the
  next tick to replay rather than demanding live delivery.

## Attribution and identity

- `src/proc/identity.rs`: agent identities are resolved without starting subprocesses.
- `src/proc/ancestors.rs`: the hook ancestor walk uses only libproc (macOS) or `/proc` (Linux);
  a test reads the module source to keep subprocess APIs out. When an ancestor exits or crosses
  into a root-owned launcher, keep the readable nearest-parent chain instead of discarding a
  found agent.
- `src/hook/attribution.rs` `apply`: a failed lookup does not prove a hook is orphaned; a direct
  child of init does.
- `tests/support/hooks.rs`: Codex auxiliary title turns (`title-notify`) are deliberately ignored
  and are not delivered evidence.
- `tests/support/transcript.rs`: once a run is hooked, only later hook payloads update its state
  and metadata; transcripts no longer do.
