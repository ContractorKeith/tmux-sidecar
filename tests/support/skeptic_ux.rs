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
