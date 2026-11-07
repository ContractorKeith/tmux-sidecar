use super::{TmuxGuard, enabled, lifecycle::list, state::rpc};
use crate::support::output;
use serde_json::json;
use std::{fs, path::Path};

e2e_test!(opencode_plugin_host_hooks_change_the_tracked_outer_run, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let binary = guard.scratch.path().join("bin/opencode.exe");
    fs::copy(guard.scratch.path().join("bin/opencode"), &binary).unwrap();
    assert!(
        output(
            guard
                .command()
                .args(["new-window", "-d"])
                .arg(&binary)
                .arg("--plugin-host")
        )
        .status
        .success()
    );
    until!(|| guard.scratch.path().join("state/plugin-host-pid").exists());
    until!(|| list(&guard).as_array().unwrap().len() == 2);
    let runs = list(&guard);
    let run = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["agent"] == "opencode")
        .unwrap();
    let child: u64 = fs::read_to_string(guard.scratch.path().join("state/plugin-host-pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(run["agent_pid"].as_u64().unwrap(), child);
    // Establish ownership, then emulate a reused server carrying a deleted pane id.
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/opencode");
    super::hooks::fire(
        &guard,
        child,
        "opencode",
        &fixtures.join("session.status.json"),
    );
    super::hooks::fire_on_pane(
        &guard,
        child,
        "opencode",
        &fixtures.join("permission.asked.json"),
        Some("%99999"),
    );
    until!(|| list(&guard)
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == run["id"] && r["state"] == "blocked"));
    super::hooks::fire(
        &guard,
        child,
        "opencode",
        &fixtures.join("session.status.json"),
    );
    super::hooks::fire(
        &guard,
        child,
        "opencode",
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hooks/opencode/permission.asked.json"),
    );
    until!(|| list(&guard)
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == run["id"] && r["state"] == "blocked"));
    assert_eq!(
        rpc(&guard, "list", json!({"all":true}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
});

e2e_test!(spool_failure_still_delivers_to_a_live_daemon, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let spool = guard.scratch.path().join("state/spool.ndjson");
    let target = guard.scratch.path().join("protected");
    fs::write(&target, b"unchanged").unwrap();
    std::os::unix::fs::symlink(&target, &spool).unwrap();
    super::cli_verbs::fire(&guard, &run, "PermissionRequest");
    until!(|| list(&guard)[0]["state"] == "blocked");
    assert_eq!(fs::read(&target).unwrap(), b"unchanged");
    assert!(
        fs::read_to_string(guard.scratch.path().join("state/hook.log"))
            .unwrap()
            .contains("attempting live delivery")
    );
});

e2e_test!(idle_accepts_send_and_unconfirmed_send_reverts, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start_capture();
    let config = "idle_after_secs=0\nsend_confirm_secs=1\n[notify]\ndesktop=false\ntmux=false\n";
    for path in [
        "config/tmux-sidecar/config.toml",
        "home/Library/Application Support/tmux-sidecar/config.toml",
    ] {
        fs::write(guard.scratch.path().join(path), config).unwrap();
    }
    let run = list(&guard)[0].clone();
    rpc(&guard, "ack", json!({"sel":run["id"]}));
    until!(|| list(&guard)[0]["state"] == "idle");
    super::cli_verbs::cli(&guard, &["send", "%0", "next prompt"], 0);
    assert_eq!(list(&guard)[0]["state"], "working");
    until!(|| list(&guard)[0]["state"] == "idle");
});

e2e_test!(agent_process_cwd_and_attention_project_root_agree, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let repo = guard.scratch.path().join("agent-project");
    fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    assert!(
        output(std::process::Command::new("git").arg("init").arg(&repo))
            .status
            .success()
    );
    assert!(
        output(
            guard
                .command()
                .args(["respawn-pane", "-k", "-t", "%0"])
                .arg(guard.scratch.path().join("bin/claude"))
                .arg(format!("--cwd={}", repo.display()))
        )
        .status
        .success()
    );
    until!(|| list(&guard)[0]["cwd"] == repo.to_str().unwrap());
    assert_eq!(
        rpc(&guard, "attention", json!({}))[0]["project_root"],
        list(&guard)[0]["project_root"]
    );
});

e2e_test!(send_errors_distinguish_before_paste_from_unknown_outcome, {
    if !enabled() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    for (phase, code) in [("load-buffer", 4), ("paste-buffer", 1), ("send-keys", 1)] {
        let guard = TmuxGuard::start_capture();
        let run = list(&guard)[0].clone();
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("tmux"))
            .find(|p| p.is_file())
            .unwrap();
        let script = guard.scratch.path().join("bin/tmux");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ \"$3\" != '{phase}' ] || exec sleep 2\nexec '{}' \"$@\"\n",
                real.display()
            ),
        )
        .unwrap();
        fs::set_permissions(script, fs::Permissions::from_mode(0o700)).unwrap();
        let result = output(guard.sidecar().args(["send", "%0", "exactly once"]));
        assert_eq!(result.status.code(), Some(code));
        let error = String::from_utf8_lossy(&result.stderr);
        if code == 1 {
            assert!(error.contains("-32007"), "{error}");
        } else {
            assert!(error.contains("-32004"), "{error}");
        }
        let input = fs::read(
            guard
                .scratch
                .path()
                .join(format!("state/input-{}", run["agent_pid"])),
        )
        .unwrap();
        if phase == "send-keys" {
            assert_eq!(input, b"\x1b[200~exactly once\x1b[201~");
        } else {
            assert!(input.is_empty());
        }
    }
});
