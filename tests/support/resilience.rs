use super::{
    enabled,
    lifecycle::{list, ok, pid, stop, until},
    support::TmuxGuard,
};
use std::{fs, os::unix::fs::PermissionsExt};
#[test]
fn two_failed_ps_ticks_keep_serving_then_recover() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps, b"#!/bin/sh\nn=0\n[ ! -f \"$HOME/ps-count\" ] || n=$(cat \"$HOME/ps-count\")\nn=$((n+1))\necho $n > \"$HOME/ps-count\"\n[ $n -gt 2 ] || exit 1\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let _ = list(&g);
    let original = pid(&g);
    until(|| list(&g).as_array().unwrap().len() == 1);
    assert_eq!(pid(&g), original);
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert!(
        log.contains("initial poll failure") && log.contains("poll failure 2"),
        "{log}"
    );
    stop(&g);
}
#[test]
fn sigterm_restart_adopts_active_run() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let before = list(&g);
    let daemon = pid(&g);
    // SAFETY: positive daemon pid belongs to this scratch fixture.
    assert_eq!(unsafe { libc::kill(daemon, libc::SIGTERM) }, 0);
    until(|| !g.scratch.path().join("run/sock").exists());
    assert_eq!(list(&g), before);
}
#[test]
fn history_is_newest_first_and_limited() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    assert!(
        g.command()
            .args(["new-window", "-d"])
            .arg(g.scratch.path().join("bin/codex"))
            .output()
            .unwrap()
            .status
            .success()
    );
    until(|| list(&g).as_array().unwrap().len() == 2);
    let result = ok(g
        .sidecar()
        .args(["list", "--all", "--limit", "1", "--json"])
        .output()
        .unwrap());
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["id"], "r2");
}

#[test]
fn five_failed_polls_exit_nonzero_with_a_log() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps, b"#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let mut daemon = super::support::ChildGuard(
        g.sidecar()
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until(|| daemon.0.try_wait().unwrap().is_some());
    assert!(!daemon.0.wait().unwrap().success());
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert!(
        log.contains("poll failure 5") && log.contains("5 consecutive poll failures"),
        "{log}"
    );
    assert!(!g.scratch.path().join("run/sock").exists());
}

#[test]
fn dropping_fixture_stops_daemon_before_the_next_poll() {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let mut daemon = super::support::ChildGuard(
        g.sidecar()
            .env("TMUX_SIDECAR_POLL_MS", "60000")
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until(|| g.scratch.path().join("run/daemon.pid").exists());
    drop(g);
    until(|| daemon.0.try_wait().unwrap().is_some());
    assert!(daemon.0.wait().unwrap().success());
}
