#[path = "support/doctor_fakes.rs"]
mod fakes;
use serde_json::Value;
use std::{fs, path::Path};

#[test]
fn installer_and_doctor_share_all_agent_home_overrides() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    fakes::fixture(home, Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    // Remove baseline hooks so only the override paths can pass doctor.
    tmux_sidecar::install::run(
        home,
        Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")),
        &[],
        false,
        false,
        false,
    )
    .unwrap();
    for path in ["custom-claude/projects", "custom-codex/sessions"] {
        fs::create_dir_all(home.join(path)).unwrap();
    }
    let command = || {
        let mut cmd = fakes::command(home);
        cmd.env("CLAUDE_CONFIG_DIR", home.join("custom-claude"))
            .env("CODEX_HOME", home.join("custom-codex"))
            .env("XDG_CONFIG_HOME", home.join("custom-config"));
        cmd
    };
    let installed = fakes::run(command().args([
        "hooks", "install", "--agent", "claude", "--agent", "codex", "--agent", "grok", "--agent",
        "opencode",
    ]));
    assert!(installed.status.success(), "{:?}", installed);
    for path in [
        "custom-claude/settings.json",
        "custom-codex/config.toml",
        "custom-config/opencode/plugins/tmux-sidecar.js",
    ] {
        assert!(home.join(path).exists(), "{path}");
    }
    let peer = fakes::peer(home, env!("CARGO_PKG_VERSION"), 1);
    let output = fakes::run(command().args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    for check in checks
        .iter()
        .filter(|c| c["name"].as_str().unwrap().starts_with("hooks "))
    {
        assert_eq!(check["status"], "ok", "{check}");
    }
    peer.join().unwrap();
}

#[test]
fn daemon_log_has_timestamp_and_nonempty_first_toml_error_line() {
    let root = tempfile::tempdir().unwrap();
    tmux_sidecar::daemon::log::write(
        root.path(),
        "\ninvalid type: string, expected a boolean\nmore detail\n",
    )
    .unwrap();
    let line = fs::read_to_string(root.path().join("daemon.log")).unwrap();
    assert!(
        line.as_bytes()[0..4].iter().all(u8::is_ascii_digit),
        "{line}"
    );
    assert_eq!(&line[10..11], "T");
    assert_eq!(&line[19..21], "Z ");
    assert!(line.contains("invalid type: string, expected a boolean"));
    assert_eq!(line.lines().count(), 1);
}

#[test]
fn version_identifies_the_git_build() {
    let root = tempfile::tempdir().unwrap();
    let output = fakes::run(fakes::command(root.path()).arg("--version"));
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("TMUX_SIDECAR_BUILD_ID")));
}

use fakes::*;

#[test]
fn broken_pipe_is_quiet_for_data_output() {
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let peer = peer(root.path(), env!("CARGO_PKG_VERSION"), 3);
    for args in [
        vec!["list", "--json"],
        vec!["history", "r1"],
        vec!["daemon", "status"],
    ] {
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        // /dev/stdout is not used: pass the closed pipe directly to the child.
        let mut cmd = command(root.path());
        cmd.args(args)
            .stdout(Stdio::from(OwnedFd::from(writer)))
            .stderr(Stdio::piped());
        struct Guard(std::process::Child);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut guard = Guard(cmd.spawn().unwrap());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(status) = guard.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        use std::io::Read;
        let mut stderr = String::new();
        guard
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.is_empty(), "{stderr}");
    }
    peer.join().unwrap();
}

#[test]
fn doctor_validates_rebased_send_confirmation_duration() {
    let root = tempfile::tempdir().unwrap();
    fakes::fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let config = if cfg!(target_os = "macos") {
        root.path()
            .join("Library/Application Support/tmux-sidecar/config.toml")
    } else {
        root.path().join(".config/tmux-sidecar/config.toml")
    };
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    for value in ["-1", "9223372036854775807"] {
        fs::write(&config, format!("send_confirm_secs={value}\n")).unwrap();
        let output = fakes::run(
            fakes::command(root.path())
                .env("XDG_CONFIG_HOME", root.path().join(".config"))
                .args(["doctor", "--json"]),
        );
        let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        let check = checks.iter().find(|c| c["name"] == "config").unwrap();
        assert_eq!(check["status"], "fail", "{check}");
        assert!(
            check["detail"]
                .as_str()
                .unwrap()
                .contains("send_confirm_secs"),
            "{check}"
        );
    }
}
