//! README hook setup before any agent or daemon has run.
use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
#[test]
fn quick_start_with_only_claude_then_doctor_succeeds() {
    let root = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    let home = root.path();
    fixture(home, Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    tmux_sidecar::install::run(
        home,
        Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")),
        &[],
        false,
        false,
        false,
    )
    .unwrap();
    for dir in [
        ".claude/projects",
        ".codex/sessions",
        ".grok/sessions",
        "run",
        "state",
    ] {
        fs::remove_dir_all(home.join(dir)).unwrap();
    }
    fs::remove_file(home.join("opencode.db")).unwrap();
    let dry = run(command(home).args(["hooks", "install", "--dry-run"]));
    assert!(dry.status.success());
    assert!(!home.join(".claude/settings.json").exists());
    let installed = run(command(home).args(["hooks", "install"]));
    assert!(installed.status.success());
    let text = String::from_utf8(installed.stdout).unwrap();
    for agent in ["codex", "grok", "opencode"] {
        assert!(text.contains(&format!("skip {agent}")), "{text}");
    }
    assert!(!home.join(".codex/config.toml").exists());
    let output = run(command(home).args(["doctor", "--json"]));
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    for name in [
        "daemon",
        "store claude",
        "state permissions",
        "runtime permissions",
    ] {
        assert_eq!(
            checks.iter().find(|c| c["name"] == name).unwrap()["status"],
            "skip"
        );
    }
    assert!(!home.join("run").exists());
    assert!(!home.join("state").exists());
}
#[test]
fn slow_tmux_does_not_recommend_installation() {
    let root = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    fs::remove_file(root.path().join("bin/tmux")).unwrap();
    fs::write(
        root.path().join("bin/tmux"),
        "#!/bin/sh\nexec /bin/sleep 3\n",
    )
    .unwrap();
    fs::set_permissions(
        root.path().join("bin/tmux"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let output = run(command(root.path())
        .env("TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER", "1")
        .args(["doctor", "--json"]));
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let check = checks.iter().find(|c| c["name"] == "tmux").unwrap();
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains("tmux slow to answer")
    );
    assert!(!check["fix"].as_str().unwrap().contains("install tmux"));
}

#[test]
fn doctor_uses_tmux_override_and_checks_state_directory_permissions() {
    let root = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    let home = root.path();
    fixture(home, Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let pinned = home.join("pinned-tmux");
    fs::rename(home.join("bin/tmux"), &pinned).unwrap();
    // Lifecycle locks and socket discovery belong beside the DB, not in runtime.
    for file in ["daemon.lock", "spawn.lock"] {
        fs::write(home.join("state").join(file), "").unwrap();
    }
    let peer = peer(home, env!("CARGO_PKG_VERSION"), 2);
    let output = run(command(home)
        .env("TMUX_SIDECAR_TMUX", &pinned)
        .args(["doctor", "--json"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::set_permissions(home.join("state"), fs::Permissions::from_mode(0o755)).unwrap();
    let output = run(command(home)
        .env("TMUX_SIDECAR_TMUX", &pinned)
        .args(["doctor", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    for (name, expected) in [
        ("tmux", "ok"),
        ("runtime permissions", "ok"),
        ("state permissions", "fail"),
    ] {
        assert_eq!(
            checks.iter().find(|c| c["name"] == name).unwrap()["status"],
            expected
        );
    }
    peer.join().unwrap();
}

#[test]
fn doctor_probe_timeout_scaling_is_test_only() {
    let root = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let tmux = root.path().join("bin/tmux");
    fs::remove_file(&tmux).unwrap();
    fs::write(&tmux, "#!/bin/sh\necho 'tmux 3.8'\nexec /bin/sleep 2\n").unwrap();
    fs::set_permissions(tmux, fs::Permissions::from_mode(0o700)).unwrap();
    let output = run(command(root.path())
        .env("TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER", "3")
        .args(["doctor", "--json"]));
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks.iter().find(|c| c["name"] == "tmux").unwrap()["status"],
        if cfg!(feature = "test-util") {
            "ok"
        } else {
            "fail"
        }
    );
}

#[test]
fn doctor_fake_executables_share_the_warmed_inodes() {
    use std::os::unix::fs::MetadataExt;
    let a = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    let b = tempfile::Builder::new()
        .prefix("tsx-dr-")
        .tempdir_in("/tmp")
        .unwrap();
    for home in [a.path(), b.path()] {
        fixture(home, Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    }
    for name in ["tmux", "notify-send", "osascript", "claude"] {
        let first = fs::metadata(a.path().join("bin").join(name)).unwrap();
        let second = fs::metadata(b.path().join("bin").join(name)).unwrap();
        assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
    }
}
