//! Doctor against fake tmux, agent stores and socket peers.
#[path = "support/doctor_fakes.rs"]
mod fakes;
use fakes::*;
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[test]
fn doctor_on_complete_and_missing_fakes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let server = peer(root.path(), env!("CARGO_PKG_VERSION"), 3);
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::remove_file(root.path().join("bin/tmux")).unwrap();
    fs::remove_file(root.path().join("bin/notify-send")).unwrap();
    fs::remove_file(root.path().join("bin/osascript")).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    for name in ["tmux", "notifier"] {
        assert_eq!(
            checks.iter().find(|c| c["name"] == name).unwrap()["status"],
            "fail"
        );
    }
    // Insecure runtime must be a reported check, not prevent all output.
    fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks
            .iter()
            .find(|c| c["name"] == "runtime permissions")
            .unwrap()["status"],
        "fail"
    );
    server.join().unwrap();
    fs::remove_file(root.path().join("run/sock")).unwrap();
    fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o700)).unwrap();
    let skew = peer(root.path(), "0.0.0", 2);
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.matches("differs from client").count(), 1);
    assert!(stderr.contains("daemon stop"));
    let output = run(command(root.path()).args(["daemon", "status", "--json"]));
    assert!(
        output.status.success(),
        "version skew must not fail ordinary clients"
    );
    assert_eq!(
        String::from_utf8(output.stderr)
            .unwrap()
            .matches("differs from client")
            .count(),
        1
    );
    skew.join().unwrap();
    fs::remove_file(root.path().join("run/sock")).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks.iter().find(|c| c["name"] == "daemon").unwrap()["status"],
        "skip"
    );
    assert!(!root.path().join("run/daemon.pid").exists());
}

#[test]
fn unused_agents_skip_but_explicit_and_partial_hooks_fail() {
    let root = tempfile::Builder::new()
        .prefix("tsx doctor é ")
        .tempdir()
        .unwrap();
    let home = root.path();
    let binary = Path::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
    fixture(home, binary);
    tmux_sidecar::install::run(home, binary, &[], false, false, false).unwrap();
    for dir in [".claude/projects", ".codex/sessions"] {
        fs::remove_dir(home.join(dir)).unwrap();
    }
    for file in [".grok/sessions/session_search.sqlite", "opencode.db"] {
        fs::remove_file(home.join(file)).unwrap();
    }
    let server = peer(home, env!("CARGO_PKG_VERSION"), 4);
    let output = run(command(home).args(["doctor", "--json"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(checks.iter().filter(|c| c["status"] == "skip").count(), 8);
    let output = run(command(home).args(["doctor", "--agent", "claude", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks.iter().find(|c| c["name"] == "hooks claude").unwrap()["status"],
        "fail"
    );
    tmux_sidecar::install::run(
        home,
        binary,
        &[tmux_sidecar::model::AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let output = run(command(home).args(["doctor", "--json"]));
    assert!(output.status.success());
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks.iter().find(|c| c["name"] == "store claude").unwrap()["status"],
        "skip"
    );
    fs::create_dir_all(home.join(".claude/projects")).unwrap();
    let output = run(command(home).arg("doctor"));
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("fix:"));
    assert!(text.contains(&home.display().to_string()));
    assert!(text.contains("skip hooks codex"));
    server.join().unwrap();
}

#[test]
fn doctor_reports_broken_config_and_database_without_writing_them() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let config = if cfg!(target_os = "macos") {
        root.path()
            .join("Library/Application Support/tmux-sidecar/config.toml")
    } else {
        root.path().join(".config/tmux-sidecar/config.toml")
    };
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "[notify]\ndesktop='yes'\n").unwrap();
    let database = root.path().join("state/tmux-sidecar.db");
    fs::write(&database, "garbage").unwrap();
    let output = run(command(root.path())
        .env("XDG_CONFIG_HOME", root.path().join(".config"))
        .args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    for (name, path) in [("config", &config), ("database", &database)] {
        let check = checks.iter().find(|c| c["name"] == name).expect(name);
        assert_eq!(check["status"], "fail");
        assert!(
            check["detail"]
                .as_str()
                .unwrap()
                .contains(path.to_str().unwrap()),
            "{check}"
        );
    }
    assert_eq!(fs::read_to_string(database).unwrap(), "garbage");
    assert!(!root.path().join("run/daemon.pid").exists());
}

#[path = "support/doctor_fresh.rs"]
mod fresh;
