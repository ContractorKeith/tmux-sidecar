use std::{fs, path::Path};
use tmux_sidecar::{install, model::AgentKind};
mod install_support;
use install_support::{put, run};
#[test]
fn round_trip_crlf_user_hooks_comments_idempotence_and_missing_files() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let json = "{\r\n  \"hooks\": {\"Stop\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"echo user\"}]}]},\r\n  \"other\": 1\r\n}\r\n";
    let toml = "# user's comment\r\nmodel = 'custom' # retain\r\n\r\n[[hooks.Stop]]\r\n[[hooks.Stop.hooks]]\r\ntype = 'command'\r\ncommand = 'echo user'\r\n";
    put(home, ".claude/settings.json", json);
    put(home, ".codex/config.toml", toml);
    let result = install_support::output(
        std::process::Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .env("HOME", home)
            .args(["hooks", "install", "--dry-run"]),
    );
    assert!(result.status.success());
    assert!(String::from_utf8(result.stdout).unwrap().contains("@@ -"));
    assert_eq!(
        fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        json
    );
    assert!(!home.join(".config").exists());
    run(home, true, false, false).unwrap();
    let installed = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
    assert!(installed.contains("# user's comment"));
    run(home, true, false, false).unwrap();
    assert_eq!(
        fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
        installed
    );
    run(home, false, false, false).unwrap();
    assert_eq!(
        fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
        json
    );
    assert_eq!(
        fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
        toml
    );
    assert!(
        !home
            .join(".config/opencode/plugins/tmux-sidecar.js")
            .exists()
    );
    assert!(!home.join(".grok/hooks/tmux-sidecar.json").exists());
    assert!(!home.join(".codex/config.toml.tmux-sidecar.bak").exists());
}
#[test]
fn blank_home_round_trip_and_grok_opt_out() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    put(home, ".grok/config.toml", "claude_hooks_enabled = false\n");
    for _ in 0..2 {
        run(home, true, false, false).unwrap();
        assert!(home.join(".grok/hooks/tmux-sidecar.json").exists());
        run(home, false, false, false).unwrap();
        for p in [
            ".claude/settings.json",
            ".codex/config.toml",
            ".grok/hooks/tmux-sidecar.json",
            ".config/opencode/plugins/tmux-sidecar.js",
        ] {
            assert!(!home.join(p).exists(), "{p}");
        }
    }
}
#[test]
fn symlink_target_and_concurrent_user_edits_survive_uninstall() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    put(home, "target.json", "{\"user\":true}\n");
    fs::create_dir(home.join(".claude")).unwrap();
    std::os::unix::fs::symlink(home.join("target.json"), home.join(".claude/settings.json"))
        .unwrap();
    install::run(
        home,
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join("target.json")).unwrap()).unwrap();
    v["new"] = true.into();
    fs::write(home.join("target.json"), serde_json::to_vec(&v).unwrap()).unwrap();
    install::run(
        home,
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        false,
        false,
        false,
    )
    .unwrap();
    assert!(home.join(".claude/settings.json").is_symlink());
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(home.join("target.json")).unwrap()).unwrap();
    assert_eq!(v, serde_json::json!({"user":true,"new":true}));
}
#[test]
fn empty_original_containers_and_inline_toml_hooks_round_trip() {
    for original in [
        "# empty hooks\n[hooks]\n",
        "[hooks]\nStop = [{hooks=[{type='command',command='echo user'}]}]\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path();
        put(home, ".codex/config.toml", original);
        put(home, ".claude/settings.json", "{\"hooks\":{\"Stop\":[]}}\n");
        run(home, true, false, false).unwrap();
        run(home, false, false, false).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
            "{\"hooks\":{\"Stop\":[]}}\n"
        );
    }
}

#[test]
fn user_comments_added_after_install_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    put(home, ".codex/config.toml", "# original\nmodel='fixture'\n");
    run(home, true, false, false).unwrap();
    let config = home.join(".codex/config.toml");
    fs::write(
        &config,
        format!(
            "# added while installed\n{}",
            fs::read_to_string(&config).unwrap()
        ),
    )
    .unwrap();
    run(home, false, false, false).unwrap();
    assert!(
        fs::read_to_string(config)
            .unwrap()
            .contains("# added while installed")
    );
}
