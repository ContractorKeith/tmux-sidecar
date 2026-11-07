use super::{enabled, support::TmuxGuard};
use crate::support::BoundedCommand;

e2e_test!(startup_toml_errors_have_a_nonempty_reason_and_timestamp, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    let config = if cfg!(target_os = "macos") {
        g.scratch
            .path()
            .join("home/Library/Application Support/tmux-sidecar/config.toml")
    } else {
        g.scratch.path().join("config/tmux-sidecar/config.toml")
    };
    std::fs::write(&config, "[notify]\ndesktop='yes'\n").unwrap();
    let output = g
        .sidecar()
        .args(["list", "--json"])
        .bounded_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("invalid type"), "{stderr}");
    assert!(!stderr.contains(": ;"), "{stderr}");
    let log = std::fs::read_to_string(g.scratch.path().join("state/daemon.log")).unwrap();
    assert_eq!(&log[10..11], "T");
    assert_eq!(&log[19..21], "Z ");
});

e2e_test!(watch_exits_quietly_when_its_output_pipe_closes, {
    if !enabled() {
        return;
    }
    use std::{
        io::Read,
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };
    let g = TmuxGuard::start();
    let run = super::lifecycle::list(&g)[0].clone();
    for json in [false, true] {
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        let mut command = g.sidecar();
        command.arg("watch");
        if json {
            command.arg("--json");
        }
        let mut child = super::support::ChildGuard::new(
            command
                .stdout(Stdio::from(OwnedFd::from(writer)))
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        until!(|| {
            super::cli_verbs::fire(&g, &run, "UserPromptSubmit");
            super::cli_verbs::fire(&g, &run, "Stop");
            child.0.try_wait().unwrap().is_some()
        });
        assert!(child.0.wait().unwrap().success());
        let mut stderr = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.is_empty(), "{stderr}");
    }
});

e2e_test!(hook_log_names_actual_agent_format_pane_and_time, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    assert!(
        g.command()
            .args(["new-window", "-d"])
            .arg(g.scratch.path().join("bin/grok"))
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| super::lifecycle::list(&g)
        .as_array()
        .unwrap()
        .iter()
        .any(|run| run["agent"] == "grok"));
    let runs = super::lifecycle::list(&g);
    let run = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["agent"] == "grok")
        .unwrap();
    let invalid = g.scratch.path().join("invalid.json");
    std::fs::write(&invalid, "invalid JSON").unwrap();
    super::hooks::fire(&g, run["agent_pid"].as_u64().unwrap(), "claude", &invalid);
    let text = std::fs::read_to_string(g.scratch.path().join("state/hook.log")).unwrap();
    let line = text.lines().find(|line| line.contains("expected")).unwrap();
    assert_eq!(&line[10..11], "T", "{line}");
    assert!(
        line.contains("agent=grok") && line.contains("format=claude"),
        "{line}"
    );
    assert!(
        line.contains(&format!("pane={}", run["pane_id"].as_str().unwrap())),
        "{line}"
    );
});

e2e_test!(desktop_notification_names_agent_project_and_pane, {
    if !enabled() {
        return;
    }
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let g = TmuxGuard::start();
    for name in ["notify-send", "osascript"] {
        let path = g.scratch.path().join("bin").join(name);
        fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$TMUX_SIDECAR_STATE_DIR/desktop.args\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let config = if cfg!(target_os = "macos") {
        g.scratch
            .path()
            .join("home/Library/Application Support/tmux-sidecar/config.toml")
    } else {
        g.scratch.path().join("config/tmux-sidecar/config.toml")
    };
    fs::write(
        config,
        "[notify]\ndesktop=true\ntmux=false\ndebounce_secs=0\n",
    )
    .unwrap();
    let project = g.scratch.path().join("named-project");
    fs::create_dir(&project).unwrap();
    let mut git = Command::new("git");
    g.isolate(&mut git);
    assert!(
        git.args(["init", "-q"])
            .arg(&project)
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        g.command()
            .args(["new-window", "-d", "-c"])
            .arg(&project)
            .arg(g.scratch.path().join("bin/grok"))
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    until!(|| super::lifecycle::list(&g)
        .as_array()
        .unwrap()
        .iter()
        .any(|run| run["agent"] == "grok"));
    let runs = super::lifecycle::list(&g);
    let run = runs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["agent"] == "grok")
        .unwrap();
    assert!(
        g.sidecar()
            .args(["name", run["pane_id"].as_str().unwrap(), "review-pane"])
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hooks/grok/PermissionRequest.json");
    super::hooks::fire(&g, run["agent_pid"].as_u64().unwrap(), "claude", &fixture);
    let recorded = g.scratch.path().join("state/desktop.args");
    until!(|| recorded.exists());
    let args = fs::read_to_string(recorded).unwrap();
    assert!(
        args.contains("grok") && args.contains("named-project") && args.contains("review-pane"),
        "{args}"
    );
});
