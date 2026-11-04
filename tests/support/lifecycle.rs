use super::{TmuxGuard, enabled};
use serde_json::Value;
use std::{
    fs,
    os::unix::{fs::MetadataExt, net::UnixStream},
    process::Output,
    thread,
    time::{Duration, Instant},
};
pub(super) fn ok(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn history(guard: &TmuxGuard) -> Value {
    ok(guard
        .sidecar()
        .args(["list", "--all", "--json"])
        .output()
        .unwrap())
}
pub(super) fn list(guard: &TmuxGuard) -> Value {
    ok(guard.sidecar().args(["list", "--json"]).output().unwrap())
}
pub(super) fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(25));
    }
}
pub(super) fn pid(guard: &TmuxGuard) -> i32 {
    fs::read_to_string(guard.scratch.path().join("run/daemon.pid"))
        .unwrap()
        .parse()
        .unwrap()
}
pub(super) fn stop(guard: &TmuxGuard) {
    assert!(
        guard
            .sidecar()
            .args(["daemon", "stop"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!guard.scratch.path().join("run/sock").exists());
}
#[test]
fn run_discovery_exit_pane_close_and_server_exit() {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    assert_eq!(
        guard
            .sidecar()
            .args(["daemon", "status"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(6)
    );
    let runs = list(&guard);
    assert_eq!(runs.as_array().unwrap().len(), 1);
    let run = &runs[0];
    assert_eq!(run["agent"], "claude");
    assert_eq!(run["pane_id"], "%0");
    assert_eq!(run["state"], "waiting");
    assert!(run["agent_pid"].as_u64().unwrap() > 0);
    assert!(run["cwd"].as_str().unwrap().starts_with('/'));
    assert_eq!(
        fs::metadata(guard.scratch.path().join("run/sock"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    // Keep the tmux server alive while %0's directly-executed agent exits.
    assert!(
        guard
            .command()
            .args(["new-window", "-d", "sleep 60"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let agent = run["agent_pid"].as_i64().unwrap() as i32;
    // SAFETY: pid came from this isolated pane; it is positive and belongs to our fake.
    assert_eq!(unsafe { libc::kill(agent, libc::SIGTERM) }, 0);
    until(|| history(&guard)[0]["state"] == "done");
    assert!(history(&guard)[0]["ended_at"].is_number());
    assert!(
        guard
            .command()
            .args(["new-window", "-d"])
            .arg(guard.scratch.path().join("bin/codex"))
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| history(&guard).as_array().unwrap().len() == 2);
    let second = history(&guard)[0]["pane_id"].as_str().unwrap().to_owned();
    assert!(
        guard
            .command()
            .args(["kill-pane", "-t", &second])
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| history(&guard)[0]["state"] == "done");
    assert!(
        guard
            .command()
            .arg("kill-server")
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| !guard.scratch.path().join("run/sock").exists());
}
#[test]
fn concurrent_clients_and_crash_recovery_keep_history() {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let commands: Vec<_> = (0..10).map(|_| guard.sidecar()).collect();
    thread::scope(|scope| {
        let handles: Vec<_> = commands
            .into_iter()
            .map(|mut cmd| scope.spawn(move || ok(cmd.args(["list", "--json"]).output().unwrap())))
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap().as_array().unwrap().len(), 1);
        }
    });
    let old_pid = pid(&guard);
    let before = list(&guard);
    let log = fs::read_to_string(guard.scratch.path().join("state/daemon.log")).unwrap();
    assert_eq!(log.matches("creating fresh database").count(), 1, "{log}");
    assert!(
        !log.contains("temporarily unavailable"),
        "racing daemon: {log}"
    );
    // SAFETY: this positive pid was read from our isolated daemon's pid file.
    assert_eq!(unsafe { libc::kill(old_pid, libc::SIGKILL) }, 0);
    until(|| UnixStream::connect(guard.scratch.path().join("run/sock")).is_err());
    let after = list(&guard);
    assert_ne!(pid(&guard), old_pid);
    assert_eq!(before[0]["id"], after[0]["id"]);
    assert_eq!(before[0]["started_at"], after[0]["started_at"]);
    assert!(guard.scratch.path().join("state/tmux-sidecar.db").exists());
    stop(&guard);
}
#[test]
fn foreground_collision_and_sigterm_cleanup() {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut child = super::support::ChildGuard(
        guard
            .sidecar()
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until(|| guard.scratch.path().join("run/daemon.pid").exists());
    let original_pid = pid(&guard);
    assert_eq!(original_pid as u32, child.0.id());
    assert!(
        !guard
            .sidecar()
            .args(["daemon", "run"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(pid(&guard), original_pid);
    assert_eq!(list(&guard).as_array().unwrap().len(), 1);
    // SAFETY: ChildGuard owns this positive pid, reaping it even if an assertion fails.
    assert_eq!(unsafe { libc::kill(original_pid, libc::SIGTERM) }, 0);
    until(|| child.0.try_wait().unwrap().is_some());
    assert!(!guard.scratch.path().join("run/sock").exists());
    assert!(!guard.scratch.path().join("run/daemon.pid").exists());
}
