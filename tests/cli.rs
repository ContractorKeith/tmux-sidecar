use std::process::Command;
#[test]
fn every_verb_is_an_explicit_stub() {
    for (args, verb, milestone) in [
        (
            vec!["list", "--group", "g", "--state", "blocked", "--json"],
            "list",
            1,
        ),
        (vec!["attention", "--json"], "attention", 4),
        (vec!["watch", "--json"], "watch", 4),
        (
            vec![
                "wait",
                "%7",
                "--until",
                "waiting",
                "--timeout",
                "600",
                "--json",
            ],
            "wait",
            4,
        ),
        (vec!["send", "r42", "-", "--force"], "send", 4),
        (vec!["ack", "r42", "%7"], "ack", 4),
        (
            vec!["history", "r42", "--limit", "10", "--json"],
            "history",
            4,
        ),
        (vec!["name", "%7", "name"], "name", 4),
        (vec!["name", "%7", "--clear"], "name", 4),
        (vec!["group", "add", "g", "%7"], "group add", 4),
        (vec!["group", "rm", "g", "%7"], "group rm", 4),
        (vec!["group", "ls", "--json"], "group ls", 4),
        (
            vec![
                "hooks",
                "install",
                "--agent",
                "claude",
                "--agent",
                "codex",
                "--dry-run",
                "--chain",
            ],
            "hooks install",
            2,
        ),
        (
            vec!["hooks", "uninstall", "--dry-run"],
            "hooks uninstall",
            2,
        ),
        (vec!["hook", "claude"], "hook", 2),
        (vec!["hook", "codex", "-payload"], "hook", 2),
        (vec!["hook", "codex-notify", "{}"], "hook", 2),
        (vec!["daemon", "status", "--json"], "daemon status", 1),
        (vec!["daemon", "stop"], "daemon stop", 1),
        (vec!["daemon", "run"], "daemon run", 1),
        (vec!["doctor", "--json"], "doctor", 7),
        (vec!["ui"], "ui", 5),
        (vec!["mcp", "--allow-send"], "mcp", 6),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .args(["--tmux-socket", "/tmp/unused", "-q"])
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{verb}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("tmux-sidecar: {verb} is not implemented yet (M{milestone})\n")
        );
    }
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
