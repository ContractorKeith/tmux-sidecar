use serde_json::json;
use std::{fs, path::Path};
use tmux_sidecar::{hook::spool, install, model::AgentKind, paths::Paths};
#[test]
fn uninstall_preserves_commands_added_inside_managed_json_entries() {
    let root = tempfile::tempdir().unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let path = root.path().join(".claude/settings.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["hooks"]["Stop"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"command","command":"echo user-added"}));
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        false,
        false,
        false,
    )
    .unwrap();
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("echo user-added")
    );
}
#[test]
fn codex_uninstall_preserves_notify_prefix_comment() {
    let original = "# my notify\nnotify=['old', 'fixed']\n";
    let installed = install::codex::edit(original, true, true, "/bin/sidecar").unwrap();
    let removed = install::codex::edit(&installed, false, false, "/bin/sidecar").unwrap();
    assert!(removed.contains("# my notify"), "{removed}");
}
#[test]
fn symlinked_spool_never_changes_target() {
    let root = tempfile::tempdir().unwrap();
    let paths = Paths {
        runtime: root.path().join("run"),
        state: root.path().join("state"),
        config: root.path().join("config"),
    };
    tmux_sidecar::paths::prepare(&paths).unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"original").unwrap();
    std::os::unix::fs::symlink(&target, paths.state.join("spool.ndjson")).unwrap();
    assert!(spool::append(&paths, b"signal\n").is_err());
    assert_eq!(fs::read(target).unwrap(), b"original");
}
#[test]
fn installers_only_register_recognized_events() {
    let root = tempfile::tempdir().unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[],
        true,
        false,
        false,
    )
    .unwrap();
    let claude = fs::read_to_string(root.path().join(".claude/settings.json")).unwrap();
    assert!(!claude.contains("StopCancelled"));
    let grok = fs::read_to_string(root.path().join(".grok/hooks/tmux-sidecar.json")).unwrap();
    let grok: serde_json::Value = serde_json::from_str(&grok).unwrap();
    assert_eq!(grok["hooks"].as_object().unwrap().len(), 1);
    assert!(grok["hooks"].get("StopCancelled").is_some());
    let codex = fs::read_to_string(root.path().join(".codex/config.toml")).unwrap();
    assert!(!codex.contains("hooks.Stop"));
    assert!(codex.contains("hooks.Interrupt"));
}
#[test]
fn parser_keeps_event_matching_independent_of_detail() {
    let parsed = tmux_sidecar::hook::signal::parse(
        "claude",
        &json!({"hook_event_name":"PreToolUse","tool_name":"Bash"}),
    )
    .unwrap();
    let event = parsed.event;
    assert_eq!(parsed.detail.as_deref(), Some("Bash"));
    assert_eq!(event, "PreToolUse");
}
#[test]
fn codex_mixed_native_and_inline_handlers_survive_uninstall() {
    for original in ["", "[hooks]\nInterrupt=[]\n"] {
        let installed = install::codex::edit(original, true, false, "/bin/sidecar").unwrap();
        let mut doc: toml_edit::DocumentMut = installed.parse().unwrap();
        let groups = doc["hooks"]["Interrupt"].as_array_of_tables_mut();
        if let Some(groups) = groups {
            let mut user = toml_edit::Table::new();
            user["type"] = toml_edit::value("command");
            user["command"] = toml_edit::value("echo user-added");
            groups.get_mut(0).unwrap()["hooks"]
                .as_array_of_tables_mut()
                .unwrap()
                .push(user);
        } else {
            let group = doc["hooks"]["Interrupt"]
                .as_array_mut()
                .unwrap()
                .get_mut(0)
                .unwrap()
                .as_inline_table_mut()
                .unwrap();
            let mut user = toml_edit::InlineTable::new();
            user.insert("type", "command".into());
            user.insert("command", "echo user-added".into());
            group
                .get_mut("hooks")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(user);
        }
        let removed = install::codex::edit(&doc.to_string(), false, false, "/bin/sidecar").unwrap();
        assert!(removed.contains("echo user-added"), "{removed}");
        assert!(!removed.contains("hook codex"));
    }
}
#[test]
fn opencode_v2_default_definition_preserves_user_suffix() {
    let installed = install::opencode::plugin("/bin/sidecar");
    assert!(installed.contains("export default"));
    assert!(installed.contains("id: \"tmux-sidecar\""));
    assert!(installed.contains("setup(ctx)"));
    assert!(installed.contains("ctx.event.subscribe"));
    let removed =
        install::opencode::edit(&(installed + "// user-added\n"), false, "/bin/sidecar").unwrap();
    assert_eq!(removed, "// user-added\n");
}
#[test]
fn codex_registers_interrupt_and_fixture_registry_excludes_unsupported_events() {
    let config = install::codex::edit("", true, false, "/bin/sidecar").unwrap();
    assert!(config.contains("hooks.Interrupt"));
    assert!(!config.contains("hooks.Stop"));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks");
    assert!(fixtures.join("codex/Interrupt.json").exists());
    assert!(!fixtures.join("codex/Stop.json").exists());
    assert!(!fixtures.join("claude/StopCancelled.json").exists());
    assert!(fixtures.join("claude/StopFailure.json").exists());
}
#[test]
fn grok_only_entry_is_separate_even_when_claude_compatibility_is_disabled() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join(".grok")).unwrap();
    fs::write(
        root.path().join(".grok/config.toml"),
        "claude_hooks_enabled=false\n",
    )
    .unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Grok],
        true,
        false,
        false,
    )
    .unwrap();
    let only: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join(".grok/hooks/tmux-sidecar.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(only["hooks"].as_object().unwrap().len(), 1);
    assert!(only["hooks"].get("StopCancelled").is_some());
    let common: serde_json::Value = serde_json::from_slice(
        &fs::read(root.path().join(".grok/hooks/tmux-sidecar-common.json")).unwrap(),
    )
    .unwrap();
    assert!(common["hooks"].get("StopCancelled").is_none());
    assert_eq!(common["hooks"].as_object().unwrap().len(), 8);
    // Re-enabling shared hooks must remove our old native common commands.
    fs::write(
        root.path().join(".grok/config.toml"),
        "claude_hooks_enabled=true\n",
    )
    .unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Grok],
        true,
        false,
        false,
    )
    .unwrap();
    let common =
        fs::read_to_string(root.path().join(".grok/hooks/tmux-sidecar-common.json")).unwrap();
    assert!(!common.contains("hook grok"));
    assert!(root.path().join(".claude/settings.json").exists());
}
