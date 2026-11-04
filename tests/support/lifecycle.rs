use super::support::stage;
use super::{TmuxGuard, enabled};
use crate::support::BoundedCommand;
use serde_json::Value;
use std::{fs, os::unix::fs::MetadataExt, process::Output, thread};
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
        .bounded_output()
        .unwrap())
}
pub(super) fn list(guard: &TmuxGuard) -> Value {
    ok(super::support::output(
        guard.sidecar().args(["list", "--json"]),
    ))
}
pub(super) fn pid(guard: &TmuxGuard) -> i32 {
    fs::read_to_string(guard.scratch.path().join("run/daemon.pid"))
        .unwrap()
        .parse()
        .unwrap()
}
pub(super) fn stop(guard: &TmuxGuard) {
    let output = super::support::output(guard.sidecar().args(["daemon", "stop"]));
    assert!(
        output.status.success(),
        "daemon stop: {}\ndaemon log: {}",
        String::from_utf8_lossy(&output.stderr),
        fs::read_to_string(guard.scratch.path().join("state/daemon.log")).unwrap_or_default(),
    );
    assert!(!guard.scratch.path().join("run/sock").exists());
}
e2e_test!(run_discovery_exit_pane_close_and_server_exit, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    assert_eq!(
        guard
            .sidecar()
            .args(["daemon", "status"])
            .bounded_output()
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
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    let agent = run["agent_pid"].as_i64().unwrap() as i32;
    // SAFETY: pid came from this isolated pane; it is positive and belongs to our fake.
    assert_eq!(unsafe { libc::kill(agent, libc::SIGTERM) }, 0);
    until!(|| history(&guard)[0]["state"] == "done");
    assert!(history(&guard)[0]["ended_at"].is_number());
    assert!(
        guard
            .command()
            .args(["new-window", "-d"])
            .arg(guard.scratch.path().join("bin/codex"))
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| history(&guard).as_array().unwrap().len() == 2);
    let second = history(&guard)[0]["pane_id"].as_str().unwrap().to_owned();
    assert!(
        guard
            .command()
            .args(["kill-pane", "-t", &second])
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| history(&guard)[0]["state"] == "done");
    assert!(
        guard
            .command()
            .arg("kill-server")
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| !guard.scratch.path().join("run/sock").exists());
});
e2e_test!(concurrent_clients_and_crash_recovery_keep_history, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    stage("concurrent recovery: starting ten clients");
    let commands: Vec<_> = (0..10).map(|_| guard.sidecar()).collect();
    let (send, receive) = std::sync::mpsc::channel();
    for mut command in commands {
        let send = send.clone();
        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                ok(super::support::output(command.args(["list", "--json"])))
            }));
            let _ = send.send(result);
        });
    }
    drop(send);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    for _ in 0..10 {
        let result = receive
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .expect("ten concurrent list clients did not finish within 10 s");
        let runs = result.unwrap_or_else(|error| std::panic::resume_unwind(error));
        assert_eq!(runs.as_array().unwrap().len(), 1);
    }
    stage("concurrent recovery: ten clients returned");
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
    until!(|| crate::support::socket::connect(guard.scratch.path().join("run/sock")).is_err());
    stage("concurrent recovery: killed daemon, requesting recovery");
    let after = list(&guard);
    stage("concurrent recovery: replacement answered");
    assert_ne!(pid(&guard), old_pid);
    assert_eq!(before[0]["id"], after[0]["id"]);
    assert_eq!(before[0]["started_at"], after[0]["started_at"]);
    assert!(guard.scratch.path().join("state/tmux-sidecar.db").exists());
    stop(&guard);
    stage("concurrent recovery: stopped replacement");
});
e2e_test!(foreground_collision_and_sigterm_cleanup, {
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
    until!(|| guard.scratch.path().join("run/daemon.pid").exists());
    let original_pid = pid(&guard);
    assert_eq!(original_pid as u32, child.0.id());
    assert!(
        !guard
            .sidecar()
            .args(["daemon", "run"])
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(pid(&guard), original_pid);
    assert_eq!(list(&guard).as_array().unwrap().len(), 1);
    // SAFETY: ChildGuard owns this positive pid, reaping it even if an assertion fails.
    assert_eq!(unsafe { libc::kill(original_pid, libc::SIGTERM) }, 0);
    until!(|| child.0.try_wait().unwrap().is_some());
    assert!(!guard.scratch.path().join("run/sock").exists());
    assert!(!guard.scratch.path().join("run/daemon.pid").exists());
});
