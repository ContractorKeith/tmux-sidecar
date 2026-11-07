use serde_json::json;
use std::{fs, os::unix::fs::symlink};
use tmux_sidecar::{model::AgentRun, paths::Paths, store::Store};

#[test]
fn state_change_carries_agent_project_and_current_pane_name() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut run: AgentRun = serde_json::from_value(json!({
        "id":"", "server_start":1, "pane_id":"%7", "agent":"grok", "agent_pid":42,
        "agent_session_id":null, "cwd":"/work/project/subdir", "project_root":"/work/project",
        "started_at":1, "ended_at":null, "state":"waiting", "state_since":1,
        "state_source":"hook:grok", "read_at":null, "last_assistant_text":"", "transcript_path":null
    }))
    .unwrap();
    run.id = store.insert(&run).unwrap();
    store.name_pane(1, "%7", Some("review")).unwrap();
    store.sent(&run, 2).unwrap();
    let changes = store.changes_since(0).unwrap();
    let event = &changes[0].1;
    assert_eq!(event["agent"], "grok");
    assert_eq!(event["project_root"], "/work/project");
    assert_eq!(event["pane_name"], "review");
}

#[test]
fn state_and_runtime_symlinks_explain_the_rejection() {
    for (field, dangling) in [
        ("state", false),
        ("runtime", false),
        ("state", true),
        ("runtime", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut paths = Paths {
            runtime: root.path().join("run"),
            state: root.path().join("state"),
            config: root.path().join("config"),
        };
        let target = root.path().join("target");
        if !dangling {
            fs::create_dir(&target).unwrap();
        }
        let alias = root.path().join("alias");
        symlink(&target, &alias).unwrap();
        if field == "state" {
            paths.state = alias.clone();
        } else {
            paths.runtime = alias.clone();
        }
        let error = tmux_sidecar::paths::prepare(&paths).unwrap_err();
        assert!(error.to_string().contains("is a symlink"), "{error}");
        assert!(
            error.to_string().contains(alias.to_str().unwrap()),
            "{error}"
        );
    }
}

#[test]
fn grok_config_parse_errors_name_the_actual_file() {
    use std::path::Path;
    use tmux_sidecar::{install, model::AgentKind};
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join(".grok/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "{broken").unwrap();
    let error = install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Grok],
        true,
        false,
        false,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(config.to_str().unwrap()),
        "{error}"
    );
}

#[test]
fn backup_parse_errors_name_the_actual_file() {
    use std::path::Path;
    use tmux_sidecar::{install, model::AgentKind};
    let root = tempfile::tempdir().unwrap();
    let settings = root.path().join(".claude/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "{}").unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let backup = settings.with_file_name("settings.json.tmux-sidecar.bak");
    fs::write(&backup, "{broken").unwrap();
    let error = install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        false,
        false,
        false,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(backup.to_str().unwrap()),
        "{error}"
    );
}
