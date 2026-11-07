use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use std::{fs, os::unix::fs::PermissionsExt};
e2e_test!(ui_receives_poll_health_warning_and_recovery, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps,b"#!/bin/sh\n[ ! -e \"$HOME/fail\" ] || { echo ui-probe-broken >&2; exit 1; }\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let _daemon = super::support::ChildGuard::new(
        g.sidecar()
            .env("TMUX_SIDECAR_POLL_MS", "1000")
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| g.scratch.path().join("run/sock").exists());
    list(&g);
    let command = format!(
        "{} --tmux-socket {} ui",
        tmux_sidecar::install::quote(env!("CARGO_BIN_EXE_tmux-sidecar")),
        tmux_sidecar::install::quote(g.socket().to_str().unwrap())
    );
    let out = output(
        g.command()
            .args(["new-window", "-d", "-P", "-F", "#{pane_id}"])
            .arg(command),
    );
    assert!(out.status.success());
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let screen = || {
        String::from_utf8(output(g.command().args(["capture-pane", "-p", "-t", &pane])).stdout)
            .unwrap()
    };
    until!(|| screen().contains("claude"));
    fs::write(g.scratch.path().join("home/fail"), b"").unwrap();
    until!(|| screen().contains("daemon can't reach tmux: ui-probe-broken"));
    fs::remove_file(g.scratch.path().join("home/fail")).unwrap();
    until!(|| !screen().contains("daemon can't reach tmux:"));
});
