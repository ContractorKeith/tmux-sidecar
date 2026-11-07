use super::{TmuxGuard, enabled, lifecycle::list};
use std::{fs, path::Path};
e2e_test!(failed_pane_pid_lookup_preserves_normal_hook_delivery, {
    if !enabled() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let cache = guard.scratch.path().join("run/pane_pids");
    fs::remove_file(&cache).unwrap();
    fs::create_dir(&cache).unwrap();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("tmux"))
        .find(|p| p.is_file())
        .unwrap();
    let script = guard.scratch.path().join("bin/tmux");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n[ \"$3\" != display-message ] || exec sleep 2\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
    super::hooks::fire(
        &guard,
        run["agent_pid"].as_u64().unwrap(),
        "claude",
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hooks/claude/PermissionRequest.json"),
    );
    fs::remove_dir(&cache).unwrap();
    until!(|| list(&guard)[0]["state"] == "blocked");
});

e2e_test!(unowned_orphan_is_dropped_and_logged_outside_sqlite, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let signal = serde_json::json!({"server_start":run["server_start"],"pane_id":"%99999","agent":"opencode","agent_pid":999999,"at":tmux_sidecar::daemon::now(),"nonce":format!("{:032x}",1),"source":"hook:opencode","event":"permission.asked","agent_session_id":"unknown","cwd":"/no-owner","orphaned":true,"payload":{}});
    super::state::rpc(&guard, "signal", signal);
    until!(
        || fs::read_to_string(guard.scratch.path().join("state/hook.log"))
            .unwrap_or_default()
            .contains("orphan hook dropped: no unambiguous")
    );
    assert_eq!(list(&guard).as_array().unwrap().len(), 1);
});
