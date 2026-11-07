use super::{
    enabled,
    lifecycle::{list, ok, pid, stop},
    support::TmuxGuard,
};
use crate::support::BoundedCommand;
use std::{fs, os::unix::fs::PermissionsExt};
e2e_test!(two_failed_ps_ticks_keep_serving_then_recover, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps, b"#!/bin/sh\nn=0\n[ ! -f \"$HOME/ps-count\" ] || n=$(cat \"$HOME/ps-count\")\nn=$((n+1))\necho $n > \"$HOME/ps-count\"\n[ $n -gt 2 ] || exit 1\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let _ = list(&g);
    let original = pid(&g);
    until!(|| list(&g).as_array().unwrap().len() == 1);
    assert_eq!(pid(&g), original);
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert!(
        log.contains("initial poll failure") && log.contains("poll failure 2"),
        "{log}"
    );
    stop(&g);
});
e2e_test!(sigterm_restart_adopts_active_run, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let before = list(&g);
    let daemon = pid(&g);
    // SAFETY: positive daemon pid belongs to this scratch fixture.
    assert_eq!(unsafe { libc::kill(daemon, libc::SIGTERM) }, 0);
    until!(|| !g.scratch.path().join("run/sock").exists());
    assert_eq!(list(&g), before);
});
e2e_test!(history_is_newest_first_and_limited, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    assert!(
        g.command()
            .args(["new-window", "-d"])
            .arg(g.scratch.path().join("bin/codex"))
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| list(&g).as_array().unwrap().len() == 2);
    let result = ok(g
        .sidecar()
        .args(["list", "--all", "--limit", "1", "--json"])
        .bounded_output()
        .unwrap());
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["id"], "r2");
});

e2e_test!(five_failed_polls_exit_nonzero_with_a_log, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps, b"#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let mut daemon = super::support::ChildGuard::new(
        g.sidecar()
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| daemon.0.try_wait().unwrap().is_some());
    assert!(
        !daemon
            .0
            .try_wait()
            .unwrap()
            .expect("daemon exit already observed within deadline")
            .success()
    );
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert!(
        log.contains("poll failure 5") && log.contains("5 consecutive poll failures"),
        "{log}"
    );
    assert!(!g.scratch.path().join("run/sock").exists());
});

e2e_test!(dropping_fixture_stops_daemon_before_the_next_poll, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let mut daemon = super::support::ChildGuard::new(
        g.sidecar()
            .env("TMUX_SIDECAR_POLL_MS", "60000")
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| g.scratch.path().join("run/daemon.pid").exists());
    drop(g);
    until!(|| daemon.0.try_wait().unwrap().is_some());
    assert!(
        daemon
            .0
            .try_wait()
            .unwrap()
            .expect("daemon exit already observed within deadline")
            .success()
    );
});

e2e_test!(spawn_lock_contention_reports_a_deadline, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let _lock =
        tmux_sidecar::client::FileLock::try_acquire(&g.scratch.path().join("run/spawn.lock"))
            .unwrap();
    let result = g
        .sidecar()
        .args(["list", "--json"])
        .bounded_output()
        .unwrap();
    assert_eq!(result.status.code(), Some(6));
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("spawn.lock") && error.contains("2 s"),
        "{error}"
    );
});
