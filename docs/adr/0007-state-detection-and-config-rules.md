# State, detection, paths and config editing rules

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003).

## Detection and state

- `src/engine/mod.rs` `detect_scan`: a pane may execute the agent directly, so `pane_pid`
  itself is a candidate. Nested agent helpers are ignored; the newest independent agent pid
  wins.
- `src/engine/state.rs` `transition`: re-reading the same completed turn must not undo a
  clock-derived idle.
- `src/store/signals.rs` `record_signal`: detached plugin hosts (unverified panes) can update a
  turn but cannot end the pane's process.
- `src/store.rs` `apply`: all changes from one snapshot commit together; a failure rolls back the
  tick. `src/store/state.rs` `change` leaves the transaction to its caller so history and run
  state become visible together.
- `src/daemon/poll.rs` `Health`: a good tick resets the failure budget; transient errors do not
  alter persisted runs.
- `src/proc.rs` `parse`: malformed process rows are transient; the rest of the snapshot is kept.
- `src/proc/liveness.rs` `running`: a zombie has exited even while its pid answers signal 0.
- `src/tmux.rs` `FORMAT`: length framing preserves tabs and newlines in cwd and command
  (session-board's idiom).
- `src/tmux/parse.rs` `parse_scan`: a row without a usable pane id fails the whole tick, since
  the previous run cannot be protected from an incomplete scan. Dead panes are authoritative
  exits even if their old pid was reused.
- `src/daemon/transcript_match.rs` `assign`: explicit identities and existing paths are reserved
  before new transcripts are assigned. A resumed transcript can predate its process, so the
  nearest start wins.
- `tests/support/round3_transcripts.rs`: session-board timestamps have second precision, so an
  interruption must land in a later second than the hook it follows.
- `src/ui/input.rs` `decode`: only key presses are decoded; editing mode owns ordinary
  characters, including q/j/k.

## Paths

- `src/paths.rs` `canonical_socket`: the absolute socket path, resolving symlinks once the
  socket exists; a not-yet-created socket keeps its absolute spelling.
- `src/paths.rs` `server_key`: FNV-1a 64-bit over the canonical socket path bytes, truncated to
  12 hex digits, so relative paths and symlinks identify the same server.
- `src/paths.rs` `resolve`: overrides name the complete per-server directory without appending
  a key. Linux without `XDG_RUNTIME_DIR` falls back to `/tmp/tmux-sidecar-<uid>/<key>`.
- `src/paths.rs` `socket_from_tmux`: the socket is the first comma-delimited field of `TMUX`,
  preserving non-UTF-8 bytes.
- `src/paths/runtime.rs` `prepare`: refuse symlinks and insecure existing directories; never
  repair an untrusted runtime.

## Hook installation

- `src/install/mod.rs`: config edits are marked and reversible, and every edit is prepared
  before the first write. Uninstall restores original bytes only when the current file is
  exactly our generated version; semantic equality alone would discard comments the user added
  after installation. Upgrades remove the marked pre-v2 OpenCode plugin at its old path.
- `src/install/files.rs` `canonical_missing`: new targets resolve existing parent symlinks just
  as existing files do.
- `src/install/locations.rs`: agent-home overrides apply to the current `HOME`; an explicit
  alternate home stays isolated from them.
- `src/cli/hooks.rs`: default installation follows PATH; explicit `--agent` and uninstall are
  deterministic.

## Doctor

- `src/doctor/mod.rs`: local diagnostics never auto-start or modify the daemon or agent stores,
  and `doctor/health.rs` never creates or migrates the history database. `run` resolves paths
  without the normal runtime security rejection so doctor can report that failure.
- `src/doctor/agents.rs`: optional agents are skipped until a managed hook marker or an explicit
  request exists.
- `src/doctor/hooks.rs` `events`: only our marked handlers are inspected; unrelated user
  handlers may appear in any order.
- `src/doctor/inspect.rs` `writable`: `access(2)` checks the current uid, ACLs and read-only
  mounts without modifying history.
