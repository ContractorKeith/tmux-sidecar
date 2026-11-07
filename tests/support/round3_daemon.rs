use super::{TmuxGuard, enabled, lifecycle::list, support::output};
use std::{
    fs,
    os::{
        fd::AsRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
};
e2e_test!(spawn_sanitizes_environment_and_closes_inherited_fds, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .unwrap();
    let wrapper = g.scratch.path().join("bin/tmux");
    fs::write(&wrapper,format!("#!/bin/sh\nprintf '%s/%s/%s' \"${{CLAUDECODE-unset}}\" \"${{TMUX_PANE-unset}}\" \"${{SKEPTIC_SECRET-unset}}\" > \"$HOME/spawn-env\"\n[ ! -e /dev/fd/200 ] || touch \"$HOME/leaked-fd\"\nexec '{}' \"$@\"\n",real.display())).unwrap();
    fs::set_permissions(wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let file = fs::File::open("/dev/null").unwrap();
    let fd = file.as_raw_fd();
    let mut cmd = g.sidecar();
    cmd.env("CLAUDECODE", "1")
        .env("TMUX_PANE", "%99")
        .env("SKEPTIC_SECRET", "private")
        .args(["list", "--json"]);
    // SAFETY: file outlives spawn, dup2 installs a deliberate inherited descriptor in our child.
    unsafe {
        cmd.pre_exec(move || {
            if libc::dup2(fd, 200) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    assert!(output(&mut cmd).status.success());
    assert_eq!(
        fs::read_to_string(g.scratch.path().join("home/spawn-env")).unwrap(),
        "unset/unset/unset"
    );
    assert!(!g.scratch.path().join("home/leaked-fd").exists());
    super::lifecycle::stop(&g);
    // Hook-triggered starts use the same launch policy.
    let run = super::binary_recovery::current_run(&g);
    super::cli_verbs::fire(&g, &serde_json::to_value(&run).unwrap(), "Stop");
    until!(|| g.scratch.path().join("run/sock").exists());
    assert_eq!(list(&g)[0]["agent"], "claude");
    assert_eq!(
        fs::read_to_string(g.scratch.path().join("home/spawn-env")).unwrap(),
        "unset/unset/unset"
    );
});
e2e_test!(spool_rejects_another_server_even_when_start_is_unknown, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let run = list(&g)[0].clone();
    super::lifecycle::stop(&g);
    let frame = serde_json::json!({"jsonrpc":"2.0","method":"signal","params":{
        "server_key":"another-server","server_start":0,"pane_id":"%0","agent":"claude","agent_pid":run["agent_pid"],
        "at":tmux_sidecar::daemon::now(),"nonce":format!("{:032x}",123),"source":"hook:claude","event":"PermissionRequest","cwd":"/fixture","payload":{}}});
    fs::write(
        g.scratch.path().join("state/spool.ndjson"),
        format!("{frame}\n"),
    )
    .unwrap();
    assert_eq!(list(&g)[0]["state"], "waiting");
    let log = fs::read_to_string(g.scratch.path().join("state/hook.log")).unwrap_or_default();
    assert!(log.contains("server key mismatch"), "{log}");
    assert!(
        output(g.sidecar().args(["history", "%0", "--json"]))
            .stdout
            .windows(17)
            .all(|s| s != b"PermissionRequest")
    );
});

e2e_test!(
    symlinked_grok_hooks_reach_the_pane_with_a_differently_named_target,
    {
        if !enabled() {
            return;
        }
        let g = TmuxGuard::start();
        let bin = g.scratch.path().join("bin");
        fs::rename(bin.join("grok"), bin.join("grok-versioned-target")).unwrap();
        std::os::unix::fs::symlink(bin.join("grok-versioned-target"), bin.join("grok")).unwrap();
        assert!(
            output(g.command().args(["new-window", "-d"]).arg(bin.join("grok")))
                .status
                .success()
        );
        until!(|| list(&g)
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["agent"] == "grok"));
        let run = list(&g)
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["agent"] == "grok")
            .unwrap()
            .clone();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hooks/grok/PermissionRequest.json");
        super::hooks::fire(&g, run["agent_pid"].as_u64().unwrap(), "claude", &fixture);
        until!(|| list(&g)
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == run["id"] && r["state"] == "blocked"));
    }
);
e2e_test!(raised_soft_fd_limit_does_not_stall_daemon_spawn, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let mut cmd = g.sidecar();
    // SAFETY: get/setrlimit operate on one initialized stack record, before exec.
    unsafe {
        cmd.pre_exec(|| {
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            if libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut limit = limit.assume_init();
            limit.rlim_cur = limit.rlim_max;
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                // macOS may advertise infinity but enforce kern.maxfilesperproc.
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINVAL) {
                    return Err(std::io::Error::last_os_error());
                }
                limit.rlim_cur = limit.rlim_max.min(10240);
                if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let start = std::time::Instant::now();
    let out = output(cmd.args(["list", "--json"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
});
e2e_test!(second_scan_error_can_confirm_the_server_is_gone, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let script = g.scratch.path().join("bin/tmux");
    fs::write(&script,"#!/bin/sh\nif [ ! -e \"$HOME/probed\" ]; then touch \"$HOME/probed\"; echo transient >&2; else echo 'no server running' >&2; fi\nexit 1\n").unwrap();
    fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
    let out = output(
        g.sidecar()
            .env_remove("TMUX_SIDECAR_TMUX")
            .args(["daemon", "run"]),
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("tmux server is not running"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
});
