# Test harness rules

Recorded from code comments removed in the 2026-11 comment audit (ADR-0003). These extend the
isolation and cleanup bullets in `AGENTS.md`.

## Isolation

- `tests/isolation/mod.rs`: every child environment is filled explicitly and never inherits a
  developer's agent paths. Rustup still needs the installed compiler, so `CARGO_HOME` and
  `RUSTUP_HOME` keep pointing at the real home while `HOME` is isolated.
- `tests/support/notify_config.rs`: tests never deliver desktop notifications to the host.
- `tests/support/no_ancestor.rs`: a detached non-agent helper keeps the test from inheriting the
  coding agent that is running it.
- `tests/support/fake_agent.rs`: the copied fake keeps a real agent `comm` while firing fixture
  hooks.
- `tests/support/readme_commands.py`: README blocks run against the binary acceptance just
  built, a healthy fake doctor setup and an isolated tmux server. Blocks that configure or
  launch the real Claude client are skipped. The release manifest fixture matches the real one:
  no `./` prefix, other platforms absent.

## Process ownership and cleanup

- `tests/support/mod.rs`: a guard is constructed before any operation that could start a
  server; `tests/support/mcp_client.rs` owns the child before any pipe operation can fail.
- `tests/support/cleanup.rs`: owned tmux servers get a graceful stop before only their recorded
  process trees are escalated. A second Drop after watchdog cleanup must not invoke tmux with a
  deleted TMPDIR. A daemon needs the original server key to accept stop, so cleanup uses the
  recorded socket. `tests/support/watchdog.rs` escalates remaining children only after
  terminal and daemon roots stop. `tests/support/leak_check.py` and `scripts/accept.sh` follow
  the same order.
- `tests/support/binary_recovery.rs`: hook-started daemons are detached, so the test takes
  ownership of their lifecycle before scratch removal.
- `tests/support/process.rs` `output`: both process exit and pipe EOF are bounded, so a detached
  descendant cannot hold up CI. `stage` writes directly to stderr so progress stays visible when
  the harness waits on a hung child. `load_timeout` keeps load scaling below the 60 s per-test
  watchdog.
- `tests/support/socket.rs` `connect`: unlike std's connect, this bounds a connect to an
  orphaned or full backlog.
- `tests/support/doctor_fakes.rs`: each fake executable is warmed before any diagnostic deadline
  starts, and the fake peer thread owns no subprocess and has deadlines on accept and reads.
- `tests/support/ui_lifecycle.rs`: the pane `exec`s the UI in the foreground so its pid is
  recorded without a background job.

## Load and timing

- `tests/support/load.rs`: ordinary process storms are bounded too, since many-core hosts would
  otherwise run every probe at once. The deliberate twenty-server storm stays separate from hook
  latency proofs. The lock guards no data, so poison is cleared and one failed test cannot fail
  the others.
- `tests/support/fake_burst.rs`: keeps 200 concurrent hooks while scaling the total deliveries.
- `tests/support/cli_deadlines.rs`: the test waits until send is probing, then fires independent
  hook evidence; hook retries cover scheduler order.
- `tests/support/mcp_replies.rs`: fake MCP replies are shared with `mcp_contract`, which checks
  them against the real daemon.

## Acceptance script

- `scripts/accept.sh`: each milestone appended live checks; never weaken an earlier one. The MCP
  check keeps stdin open through initialize and tools/list, because EOF cancels SDK requests.
