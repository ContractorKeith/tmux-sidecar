# Daemon lifecycle, locks and descriptors

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003). Paths and
functions are where each decision is implemented.

## Spawning and ownership

- `src/client/spawn.rs` `StartingChild`: binding the socket hands lifecycle ownership to the
  daemon. Cancellation before that point sends SIGTERM and reaps asynchronously so the child
  can clean up normally. Dropping the `Child` relies on Tokio's Unix orphan queue to reap it
  after SIGCHLD: there is no waiting thread, and `kill_on_drop` is false for a handed-over
  daemon.
- `src/client/spawn.rs` `ensure`: a detached hook trigger can win the lifetime lock after our
  connect. Join that startup rather than reporting the losing child's lock error.
- `src/client/trigger.rs` `trigger_spawn`: daemon lifetime locking arbitrates concurrent
  triggers; the hook never waits for startup.
- `src/client.rs` `identified`: internal clients leave startup warnings for a client that may
  auto-spawn, which reports them.
- `src/client/stop.rs` `stop`: stop is idempotent and waits on the endpoint of the verified
  connection.
- `tests/support/resilience.rs`: binding the socket precedes PID publication, so readers wait
  for a complete PID value.

## Locks

- `src/client/spawn.rs` `FileLock`: a lock lives as long as its file; CLOEXEC prevents a
  spawned daemon inheriting it.
- `src/daemon/mod.rs` `run_inner`: the lifetime lock also protects foreground starts; a start
  never unlinks a live daemon's socket.
- `src/daemon/startup.rs` `legacy_lock`: the daemon also holds the legacy runtime-dir lock for
  its lifetime, so an older binary cannot start beside it.
- `tests/support/doctor_fresh.rs`: lifecycle locks and socket discovery belong beside the DB in
  the state dir, not in the runtime dir.

## Descriptors

- `src/client/fds.rs`: inherited-descriptor cleanup before exec is bounded, including on older
  Linux kernels without `close_range`.
- `src/client/launch.rs` `prepare`: `getdtablesize` can follow an agent's raised soft limit, so
  the fallback loop is bounded by `fds::limit`.
- `src/daemon/accept.rs`: connections are bounded and descriptor exhaustion never ends the
  daemon. On EMFILE the daemon frees one spare descriptor so the failure itself can be logged,
  then reopens it.
- `src/daemon/peer.rs` `disconnected`: EOF alone is a write-half close; only POLLHUP identifies a
  fully disconnected peer.

## Recovery, shutdown and logs

- `src/daemon/recovery.rs`: a startup marker distinguishes a missing history DB from a first
  installation. `report` runs only after hello validates the server; hooks use `verify()` and
  never this output path.
- `src/daemon/shutdown.rs`: signals already accepted are finished before SQLite closes; idle
  clients get a deadline.
- `src/daemon/log.rs`: diagnostics are size-bounded and each write reopens the file after
  rotation. `timestamp` formats UTC RFC 3339 with `gmtime_r`, shared with hook diagnostics,
  to avoid a date-formatting dependency.
