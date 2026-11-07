use super::{
    TmuxGuard, enabled,
    lifecycle::list,
    support::{ChildGuard, output},
};
use std::{
    fs,
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    process::{Command, Stdio},
};

e2e_test!(legacy_daemon_lock_preserves_live_socket, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let _lock =
        tmux_sidecar::client::FileLock::try_acquire(&g.scratch.path().join("run/daemon.lock"))
            .unwrap();
    let _listener = UnixListener::bind(g.scratch.path().join("run/sock")).unwrap();
    let inode = fs::metadata(g.scratch.path().join("run/sock"))
        .unwrap()
        .ino();
    let mut command = Command::new("sleep");
    g.isolate(&mut command);
    let old = ChildGuard::new(command.arg("60").spawn().unwrap());
    fs::write(
        g.scratch.path().join("run/daemon.pid"),
        old.0.id().to_string(),
    )
    .unwrap();
    let result = output(g.sidecar().args(["daemon", "run"]));
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains(&format!(
            "an older tmux-sidecar daemon (pid {}) is running",
            old.0.id()
        )),
        "{error}"
    );
    assert_eq!(
        fs::metadata(g.scratch.path().join("run/sock"))
            .unwrap()
            .ino(),
        inode
    );
});

fn fail_polls(g: &TmuxGuard) {
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps,b"#!/bin/sh\n[ ! -e \"$HOME/fail\" ] || { echo probe-broken >&2; exit 1; }\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
}
e2e_test!(stale_rows_warn_humans_and_refuse_actions, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start_capture();
    fail_polls(&g);
    let run = list(&g)[0].clone();
    super::cli_verbs::fire(&g, &run, "Stop");
    // Keep the stale interval long enough to inspect all surfaces before shutdown.
    super::lifecycle::stop(&g);
    let _daemon = ChildGuard::new(
        g.sidecar()
            .env("TMUX_SIDECAR_POLL_MS", "2000")
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| g.scratch.path().join("run/sock").exists());
    fs::write(g.scratch.path().join("home/fail"), b"").unwrap();
    until!(|| list(&g)[0]["stale"] == true);
    for verb in ["list", "attention"] {
        let result = output(g.sidecar().arg(verb));
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("waiting STALE"), "{text}");
        assert!(
            text.contains("daemon can't reach tmux: probe-broken"),
            "{text}"
        );
    }
    for args in [
        vec!["wait", "%0", "--until", "waiting", "--timeout", "1"],
        vec!["send", "%0", "unsafe", "--force"],
    ] {
        let result = output(g.sidecar().args(args));
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("daemon can't reach tmux: probe-broken")
        );
    }
    super::state::rpc(&g, "ack", serde_json::json!({"sel":"%0"}));
    let empty = output(g.sidecar().arg("attention"));
    assert!(
        String::from_utf8_lossy(&empty.stdout).contains("daemon can't reach tmux: probe-broken")
    );
    fs::remove_file(g.scratch.path().join("home/fail")).unwrap();
    until!(|| list(&g)[0]["stale"] == false);
});

e2e_test!(invalid_pinned_tmux_falls_back_and_logs_once, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let result = output(
        g.sidecar()
            .env("TMUX_SIDECAR_TMUX", g.scratch.path().join("missing-tmux"))
            .args(["list", "--json"]),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    list(&g);
    list(&g);
    let log = fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert_eq!(log.matches("falling back to PATH").count(), 1, "{log}");
});
