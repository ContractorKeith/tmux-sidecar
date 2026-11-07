#[path = "install_support/process.rs"]
mod support;
use std::{fs, path::Path, process::Command};
use tmux_sidecar::{install, model::AgentKind, store::Store};

#[test]
fn json_order_survives_install_user_edit_uninstall() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{\"zebra\":1,\"theme\":\"dark\",\"alpha\":2}\n").unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap().replace("dark", "light");
    fs::write(&path, text).unwrap();
    install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        false,
        false,
        false,
    )
    .unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.find("zebra").unwrap() < text.find("theme").unwrap(),
        "{text}"
    );
    assert!(
        text.find("theme").unwrap() < text.find("alpha").unwrap(),
        "{text}"
    );
    assert!(
        !path
            .with_file_name("settings.json.tmux-sidecar.bak")
            .exists()
    );
}

#[test]
fn newer_schema_requests_upgrade_without_discarding_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    drop(Store::open(&path).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("UPDATE meta SET schema_version=99", [])
        .unwrap();
    let error = Store::open(&path).err().unwrap().to_string();
    assert!(
        error.contains("database schema 99 is newer than this binary (3); upgrade tmux-sidecar"),
        "{error}"
    );
    assert!(!error.contains("move it aside"), "{error}");
}

#[test]
fn settings_parse_errors_name_the_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".claude/settings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{broken").unwrap();
    let error = install::run(
        root.path(),
        Path::new("/bin/sidecar"),
        &[AgentKind::Claude],
        true,
        false,
        false,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains(path.to_str().unwrap()),
        "{error}"
    );
}

#[test]
fn absent_daemon_status_and_stop_are_sentences() {
    let root = tempfile::tempdir().unwrap();
    for verb in ["status", "stop"] {
        let output = support::output(
            Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
                .env("HOME", root.path())
                .args([
                    "--tmux-socket",
                    root.path().join("tmux").to_str().unwrap(),
                    "daemon",
                    verb,
                ]),
        );
        assert_eq!(output.status.code(), Some(6));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("daemon not running"),
            "{:?}",
            output
        );
    }
}

#[test]
fn dry_run_is_a_patch_for_new_and_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join(".codex/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "# retain this line\nmodel='test'\n").unwrap();
    let output = support::output(
        Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .env("HOME", root.path())
            .args(["hooks", "install", "--dry-run"]),
    );
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("@@ -0,0 +1,"), "{text}");
    assert!(!text.contains("-# retain this line"), "{text}");
    let patch = root.path().join("changes.patch");
    // Informational installer output follows the final hunk.
    fs::write(&patch, text).unwrap();
    for dir in [".claude", ".grok/hooks", ".config/opencode/plugins"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    let applied = support::output(
        Command::new("patch")
            .current_dir(root.path())
            .args(["--dry-run", "--batch"])
            .arg(format!("-p{}", root.path().components().count()))
            .arg("-i")
            .arg(&patch),
    );
    assert!(
        applied.status.success(),
        "{} {}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
}

#[test]
fn closed_stdout_exits_quietly_for_install_and_doctor() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    use std::process::Stdio;
    for args in [
        vec!["hooks", "install", "--dry-run"],
        vec!["doctor", "--json"],
        vec!["--help"],
        vec!["--version"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        let mut command = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
        command
            .env("HOME", root.path())
            .args(["--tmux-socket", root.path().join("tmux").to_str().unwrap()])
            .args(args);
        let _sandbox = support::isolation::fill(&mut command);
        command
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::piped());
        let mut guard = support::ChildGuard(command.spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while guard.0.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(guard.0.wait().unwrap().success());
        let mut stderr = String::new();
        use std::io::Read;
        guard
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.is_empty(), "{stderr}");
    }
}
