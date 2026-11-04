use std::{fs, process::Command};
mod install_support;
use install_support::{put, run};
#[test]
fn notify_collision_is_preflighted_and_chain_preserves_argv() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let output = home.join("argv");
    let script = home.join("old notify");
    put(
        home,
        "old notify",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
            output.display()
        ),
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let original = format!(
        "notify = [{}, 'fixed arg'] # original\n",
        serde_json::to_string(script.to_str().unwrap()).unwrap()
    );
    put(home, ".codex/config.toml", &original);
    assert!(
        run(home, true, false, false)
            .unwrap_err()
            .to_string()
            .contains("--chain")
    );
    assert!(!home.join(".claude").exists());
    run(home, true, true, false).unwrap();
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(home.join(".codex/config.toml")).unwrap()).unwrap();
    let args = config["notify"].as_array().unwrap();
    let result = install_support::output(
        Command::new(args[0].as_str().unwrap())
            .args(args.iter().skip(1).map(|a| a.as_str().unwrap()))
            .arg("{\"type\":\"agent-turn-complete\"}")
            .env("HOME", home)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE"),
    );
    assert!(result.status.success());
    assert_eq!(
        fs::read_to_string(output).unwrap(),
        "fixed arg\n{\"type\":\"agent-turn-complete\"}\n"
    );
    run(home, false, false, false).unwrap();
    assert_eq!(
        fs::read_to_string(home.join(".codex/config.toml")).unwrap(),
        original
    );
}
