# Linux, macOS and BSD differences

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003). Each item is a
platform behaviour the code or tests depend on.

## Production

- `src/daemon/peer.rs` `disconnected`: Darwin `poll` registers only the requested filters, so
  the probe requests POLLOUT to observe a fully closed peer while still treating a write-half
  close as connected.
- `src/doctor/inspect.rs` `tmux_version`: OpenBSD reports `openbsd-<release>`; base-system
  tmux is at least 3.3 from OpenBSD 7.3.
- `src/proc/identity.rs` `mac_names`: `KERN_PROCARGS2` returns argc, the executable path, NUL
  padding, then argv[0].
- `src/proc/liveness.rs` `linux_stat`: a process can disappear after `kill(0)` or after its
  `/proc` file opens; both ENOENT and ESRCH mean exited.

## Tests

- macOS sockets accepted from a nonblocking listener inherit O_NONBLOCK; Linux sockets do not.
  Fake peers set blocking mode explicitly (`tests/support/doctor_fakes.rs`,
  `hooks_identity.rs`, `mcp_rpc.rs`).
- macOS scans each fresh unsigned executable on first exec. Fake agents are built once per
  test process and every agent name is a hard link to that one inode, keeping both the name
  and the scanned inode (`tests/support/fake_binary.rs`). The test process never opens a fake
  for writing, so a concurrent fork cannot inherit a writable descriptor and make exec fail
  with ETXTBSY (#64).
- Darwin can reject socket-option changes after the daemon closes an over-capacity peer, so
  connection-limit tests keep deadlines in the tokio runtime instead of socket options. Darwin
  also refuses a full backlog rather than waiting for accept
  (`tests/support/connection_limits.rs`).
- macOS PTY output buffers are small: the panic test drains the master while the child runs, or
  the child's diagnostic can block its exit. macOS also revokes the slave when its controlling
  session exits, so the chained hook records terminal modes before exit
  (`src/ui/terminal_tests.rs`).
- APFS rejects invalid UTF-8 at mkdir; the live byte-cwd test skips on macOS while parser
  coverage still runs (`tests/support/round1.rs`).
- macOS may advertise an infinite fd limit but enforce `kern.maxfilesperproc`
  (`tests/support/round3_daemon.rs`).
- Darwin `ps -E` includes the environment; macOS reports `/tmp` in process environments and
  `/private/tmp` from realpath (`tests/support/leak_check.py`).
- BSD reports EIO if the PTY slave closes between poll and write; the README runner relies on
  the exit-status assertion instead (`tests/support/readme_commands.py`).
- `scripts/accept.sh` stays Bash 3.2 compatible for macOS: no `mapfile`, no `${var,,}`, no
  associative arrays. A just-exited orphan daemon can briefly be a zombie, so the stop check
  inspects its command line rather than `kill -0` alone.
