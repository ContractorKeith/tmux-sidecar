use super::{TmuxGuard, enabled};
use crate::support::BoundedCommand;
use serde_json::Value;
use std::{fs, path::Path};
pub(super) fn fire(guard: &TmuxGuard, pid: u64, format: &str, fixture: &Path) {
    fire_on_pane(guard, pid, format, fixture, None);
}
pub(super) fn fire_on_pane(
    guard: &TmuxGuard,
    pid: u64,
    format: &str,
    fixture: &Path,
    pane: Option<&str>,
) {
    let state = guard.scratch.path().join("state");
    let done = state.join(format!("fired-{pid}"));
    let _ = fs::remove_file(&done);
    fs::write(
        state.join(format!("fire-{pid}.new")),
        format!(
            "{}\n{format}\n{}{}",
            env!("CARGO_BIN_EXE_tmux-sidecar"),
            fixture.display(),
            pane.map(|p| format!("\n{p}")).unwrap_or_default()
        ),
    )
    .unwrap();
    fs::rename(
        state.join(format!("fire-{pid}.new")),
        state.join(format!("fire-{pid}")),
    )
    .unwrap();
    until!(|| done.exists());
    let result = fs::read_to_string(done).unwrap();
    let fields: Vec<_> = result.lines().collect();
    assert_eq!(fields[0], "true");
    let elapsed = fields[1].parse::<u64>().unwrap();
    crate::support::stage(&format!(
        "hook {format} {}: {elapsed} ms (e2e wall clock)",
        fixture.display()
    ));
    // Shared-runner scheduling is outside the hook's tested internal deadline.
    assert!(elapsed < 1000, "hook wall clock exceeded 1000 ms: {result}");
    assert_eq!(fields[2], "0");
    assert_eq!(fields[3], "0");
}
fn inspect(guard: &TmuxGuard) -> Vec<Value> {
    tmux_sidecar::store::Store::open(&guard.scratch.path().join("state/tmux-sidecar.db"))
        .unwrap()
        .signal_history()
        .unwrap()
}
e2e_test!(every_fixture_is_delivered_by_its_real_ancestor, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    for agent in ["codex", "grok", "opencode"] {
        assert!(
            guard
                .command()
                .args(["new-window", "-d"])
                .arg(guard.scratch.path().join("bin").join(agent))
                .bounded_output()
                .unwrap()
                .status
                .success()
        );
    }
    until!(|| super::lifecycle::list(&guard).as_array().unwrap().len() == 4);
    let runs = super::lifecycle::list(&guard);
    let mut count = 0;
    for run in runs.as_array().unwrap() {
        let agent = run["agent"].as_str().unwrap();
        let pid = run["agent_pid"].as_u64().unwrap();
        let mut files: Vec<_> = fs::read_dir(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/hooks")
                .join(agent),
        )
        .unwrap()
        .map(|f| f.unwrap().path())
        .collect();
        files.sort();
        for fixture in files {
            // Auxiliary title turns are intentionally ignored, not delivered evidence.
            if fixture.file_stem().unwrap() == "title-notify" {
                continue;
            }
            let format = if fixture.file_stem().unwrap() == "notify" {
                "codex-notify"
            } else if agent == "grok" {
                "claude"
            } else {
                agent
            };
            fire(&guard, pid, format, &fixture);
            count += 1;
        }
    }
    // A busy daemon may legitimately make a hook spool; allow its next tick to replay.
    let spool = guard.scratch.path().join("state/spool.ndjson");
    until!(|| tmux_sidecar::hook::spool::drained(spool.parent().unwrap()).unwrap());
    super::lifecycle::stop(&guard);
    let signals = inspect(&guard);
    assert_eq!(
        signals.len(),
        count,
        "hook log: {}\ndaemon log: {}\nsignals: {signals:?}",
        fs::read_to_string(guard.scratch.path().join("state/hook.log")).unwrap_or_default(),
        fs::read_to_string(guard.scratch.path().join("state/daemon.log")).unwrap_or_default()
    );
    for run in runs.as_array().unwrap() {
        let matching: Vec<_> = signals
            .iter()
            .filter(|s| s["run_id"] == run["id"])
            .collect();
        assert!(!matching.is_empty());
        if run["agent"] == "grok" {
            assert!(matching.iter().all(|s| s["source"] == "hook:claude"));
        }
    }
});
e2e_test!(down_daemon_spools_replays_in_order_and_deduplicates, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let runs = super::lifecycle::list(&guard);
    let pid = runs[0]["agent_pid"].as_u64().unwrap();
    super::lifecycle::stop(&guard);
    // Hold the daemon lifetime lock to exercise spooling without racing auto-start.
    let lock =
        tmux_sidecar::client::FileLock::try_acquire(&guard.scratch.path().join("run/daemon.lock"))
            .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude");
    fire(
        &guard,
        pid,
        "claude",
        &fixtures.join("UserPromptSubmit.json"),
    );
    fire(&guard, pid, "claude", &fixtures.join("Stop.json"));
    let path = guard.scratch.path().join("state/spool.ndjson");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        bytes
            .split(|b| *b == b'\n')
            .filter(|b| !b.is_empty())
            .count(),
        2
    );
    drop(lock);
    let _ = super::lifecycle::list(&guard);
    until!(|| tmux_sidecar::hook::spool::drained(path.parent().unwrap()).unwrap());
    // Simulate a crash between committing the records and acknowledging the spool.
    fs::write(&path, &bytes).unwrap();
    until!(|| tmux_sidecar::hook::spool::drained(path.parent().unwrap()).unwrap());
    super::lifecycle::stop(&guard);
    let signals = inspect(&guard);
    assert_eq!(signals.len(), 2);
    assert_eq!(signals[0]["event"], "UserPromptSubmit");
    assert_eq!(signals[1]["event"], "Stop");
});
e2e_test!(outside_tmux_or_without_an_agent_records_nothing, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    for inside in [false, true] {
        let mut cmd = guard.sidecar();
        if inside {
            cmd.env("TMUX", format!("{},0,0", guard.socket().display()))
                .env("TMUX_PANE", "%0");
        }
        let result = cmd.args(["hook", "claude"]).bounded_output().unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
    assert!(!guard.scratch.path().join("state/spool.ndjson").exists());
    assert!(!guard.scratch.path().join("state/tmux-sidecar.db").exists());
    let log = fs::read_to_string(guard.scratch.path().join("state/hook.log")).unwrap();
    assert_eq!(log.lines().count(), 1);
    // The test runner itself can have a real coding-agent ancestor. In that case
    // the empty payload is rejected; a truly agent-free walk logs its missing ancestor.
    assert!(
        log.contains("no agent ancestor") || log.contains("EOF while parsing"),
        "{log}"
    );
});
