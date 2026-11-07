use super::{
    TmuxGuard, enabled,
    lifecycle::{list, ok, pid},
    support::output,
};
use std::{fs, os::unix::fs::PermissionsExt};

e2e_test!(different_runtime_reuses_state_owner, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let before = list(&g);
    let owner = pid(&g);
    let other = g.scratch.path().join("other-runtime");
    let result = output(
        g.sidecar()
            .env("TMUX_SIDECAR_RUNTIME_DIR", &other)
            .args(["list", "--json"]),
    );
    assert_eq!(ok(result), before);
    assert!(
        !other.join("daemon.pid").exists(),
        "second daemon was spawned"
    );
    assert_eq!(pid(&g), owner);
    assert!(
        fs::read_to_string(g.scratch.path().join("state/daemon.socket"))
            .unwrap()
            .contains("sock")
    );
});

e2e_test!(process_only_send_is_refused, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start_capture();
    let run = list(&g)[0].clone();
    let result = output(g.sidecar().args(["send", "%0", "unsafe"]));
    assert_eq!(result.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&result.stderr).contains("no hook evidence yet"));
    let mut mcp = super::mcp_client::Mcp::start(&g, true);
    let result = mcp.call(
        "send_prompt",
        serde_json::json!({"selector":"%0","text":"unsafe"}),
    );
    assert_eq!(result["isError"], true);
    assert!(result.to_string().contains("no hook evidence yet"));
    assert!(
        fs::read(
            g.scratch
                .path()
                .join(format!("state/input-{}", run["agent_pid"]))
        )
        .unwrap()
        .is_empty()
    );
    super::cli_verbs::cli(&g, &["send", "%0", "forced", "--force"], 0);
});

e2e_test!(failed_poll_marks_rows_stale_then_recovers, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(
        &ps,
        b"#!/bin/sh\n[ ! -e \"$HOME/fail\" ] || exit 1\nexec /bin/ps \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&ps, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(list(&g)[0]["stale"], false);
    fs::write(g.scratch.path().join("home/fail"), b"").unwrap();
    until!(|| list(&g)[0]["stale"] == true);
    fs::remove_file(g.scratch.path().join("home/fail")).unwrap();
    until!(|| list(&g)[0]["stale"] == false);
});

e2e_test!(spawn_passes_absolute_tmux_and_missing_tmux_is_clear, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let result = output(
        g.sidecar()
            .env("PATH", "/missing")
            .env("TMUX_SIDECAR_TMUX", &real)
            .args(["list", "--json"]),
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    super::lifecycle::stop(&g);
    let result = output(
        g.sidecar()
            .env("PATH", "/missing")
            .env_remove("TMUX_SIDECAR_TMUX")
            .args(["list", "--json"]),
    );
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("tmux not found (looked in PATH=/missing)")
    );
});

e2e_test!(client_and_hook_spawn_pass_the_resolved_tmux_path, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .unwrap();
    let wrapper = g.scratch.path().join("bin/tmux");
    fs::write(&wrapper, format!("#!/bin/sh\nprintf '%s' \"$TMUX_SIDECAR_TMUX\" > \"$HOME/resolved-tmux\"\nexec '{}' \"$@\"\n", real.display())).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
    let run = list(&g)[0].clone();
    let marker = g.scratch.path().join("home/resolved-tmux");
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        wrapper.to_str().unwrap()
    );
    super::lifecycle::stop(&g);
    fs::remove_file(&marker).unwrap();
    super::cli_verbs::fire(&g, &run, "Stop");
    until!(|| g.scratch.path().join("run/sock").exists());
    until!(|| fs::read_to_string(&marker).unwrap_or_default() == wrapper.to_str().unwrap());
});
