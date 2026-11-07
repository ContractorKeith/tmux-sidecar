use std::process::Command;
#[test]
fn doctor_quiet_still_reports_failure_exit_code() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
        .env("HOME", root.path())
        .env("CLAUDE_CONFIG_DIR", root.path().join(".claude"))
        .env("CODEX_HOME", root.path().join(".codex"))
        .env("GROK_HOME", root.path().join(".grok"))
        .env("OPENCODE_DB", root.path().join("opencode.db"))
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
        .args(["--tmux-socket", "/tmp/unused", "-q", "doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("doctor found"));
}
#[test]
fn help_and_version_work() {
    for arg in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .arg(arg)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        if arg == "--help" {
            let help = String::from_utf8(output.stdout).unwrap();
            for verb in [
                "list",
                "attention",
                "watch",
                "wait",
                "send",
                "ack",
                "history",
                "name",
                "group",
                "hooks",
                "hook",
                "daemon",
                "doctor",
                "ui",
                "mcp",
            ] {
                assert!(
                    help.lines()
                        .any(|line| line.split_whitespace().next() == Some(verb)),
                    "missing {verb} in help"
                );
            }
        }
    }
}
#[test]
fn nested_help_lists_every_subcommand() {
    for (parent, verbs) in [
        ("group", vec!["add", "rm", "ls"]),
        ("hooks", vec!["install", "uninstall"]),
        ("daemon", vec!["status", "stop", "run"]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .args([parent, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8(output.stdout).unwrap();
        for verb in verbs {
            assert!(
                help.lines()
                    .any(|line| line.split_whitespace().next() == Some(verb)),
                "missing {parent} {verb}"
            );
        }
    }
}
#[test]
fn fakes_are_available_to_integration_tests() {
    use tmux_sidecar::seams::{Tmux, fakes::FakeTmux};
    assert!(FakeTmux::default().list_panes().unwrap().is_empty());
}

#[test]
fn missing_default_server_is_clear_and_never_spawns() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", root.path())
        .env("HOME", root.path())
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
        .args(["list", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("no tmux server")
    );
    assert!(!root.path().join("run").exists());
}
#[test]
fn m4_argument_contract() {
    use clap::Parser;
    use tmux_sidecar::cli::{Cli, Command as Verb};
    for args in [
        vec!["send", "%0", "--force", "hello"],
        vec!["send", "%0", "hello", "--force"],
        vec!["send", "%0", "-", "--force"],
    ] {
        let parsed = Cli::try_parse_from(std::iter::once("tmux-sidecar").chain(args)).unwrap();
        assert!(matches!(parsed.command, Verb::Send { force: true, .. }));
    }
    for args in [
        vec!["wait", "%0", "--until", "working"],
        vec!["history", "%0", "--limit", "4294967296"],
        vec!["name", "%0", "x", "--clear"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("tmux-sidecar").chain(args)).is_err());
    }
}
#[test]
fn empty_prompts_fail_before_connecting() {
    for text in ["", "-"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .args(["--tmux-socket", "/no/tsx-server", "send", "%0", text])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("empty prompt"));
    }
}
#[test]
fn escaped_prompt_size_is_checked_before_connecting() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("prompt");
    std::fs::write(&path, "\"".repeat(600_000)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
        .args(["--tmux-socket", "/no/tsx-server", "send", "%0", "-"])
        .stdin(std::fs::File::open(path).unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("prompt exceeds maximum 1 MiB encoded request frame")
    );
    let help = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
        .args(["send", "--help"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("send %0 -- -text"));
}
