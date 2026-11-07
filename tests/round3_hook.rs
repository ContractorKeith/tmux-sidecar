use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[path = "install_support/process.rs"]
mod process;
#[test]
fn open_stdin_never_holds_hooks_past_the_budget() {
    let root = tempfile::tempdir().unwrap();
    for (format, tmux) in [("claude", false), ("claude", true), ("codex-notify", true)] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
        cmd.args(["hook", format])
            .env_remove("TMUX")
            .env("HOME", root.path())
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if tmux {
            cmd.env(
                "TMUX",
                format!("{},1,0", root.path().join("absent").display()),
            )
            .env("TMUX_PANE", "%0");
        }
        let mut child = process::ChildGuard(cmd.spawn().unwrap());
        // Keep the pipe's writer alive, including after sending a complete JSON document.
        use std::io::Write;
        child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"{\"hook_event_name\":\"Stop\"}")
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{format}: open stdin exceeded 200 ms"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
#[test]
fn missing_or_unknown_hook_arguments_are_a_quiet_success() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["hook"],
        vec!["hook", "bogus"],
        vec!["hook", "--bogus"],
        vec!["hook", "bogus", "extra", "unknown"],
    ] {
        let out = process::output(
            Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
                .args(&args)
                .env_remove("TMUX")
                .env("HOME", root.path())
                .stdin(Stdio::null()),
        );
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
}
