use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tmux_sidecar::hook::{self, input, spool};
#[path = "install_support/process.rs"]
mod process;
use process::{ChildGuard as Child, output};
#[test]
fn five_megabyte_payload_has_bounded_marked_preview() {
    let original = serde_json::json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_response":{"stdout":"x".repeat(5*1024*1024)}});
    let compact = input::compact(original).unwrap();
    assert!(serde_json::to_vec(&compact).unwrap().len() < 256 * 1024);
    assert_eq!(compact["_tmux_sidecar_truncated"], true);
    assert!(
        compact["tool_response"]["stdout"]
            .as_str()
            .unwrap()
            .ends_with("…truncated")
    );
}
#[test]
fn log_and_rejected_spool_refuse_symlinks_and_fifo() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"original").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join("hook.log")).unwrap();
    std::os::unix::fs::symlink(&target, root.path().join("spool.rejected")).unwrap();
    assert!(hook::log(root.path(), &"lost").is_err());
    assert!(spool::reject(root.path(), b"bad\n").is_err());
    assert_eq!(fs::read(&target).unwrap(), b"original");
    fs::remove_file(root.path().join("hook.log")).unwrap();
    let name = std::ffi::CString::new(root.path().join("hook.log").as_os_str().as_encoded_bytes())
        .unwrap();
    // SAFETY: name is NUL-terminated and points into an owned temporary directory.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(hook::log(root.path(), &"lost").is_err());
}
#[test]
fn no_ancestor_still_drains_five_megabytes_without_epipe() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Child(
        Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .args(["hook", "claude"])
            .env("HOME", root.path())
            .env("TMUX", "/tmp/tsx-no-server,1,0")
            .env("TMUX_PANE", "%0")
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut stdin = child.0.stdin.take().unwrap();
    stdin
        .write_all(&vec![b'x'; 5 * 1024 * 1024])
        .expect("hook closed stdin before EOF");
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!root.path().join("state/spool.ndjson").exists());
}
#[test]
fn clap_hook_help_and_unknown_format_are_consistent() {
    let root = tempfile::tempdir().unwrap();
    let help = output(Command::new(env!("CARGO_BIN_EXE_tmux-sidecar")).args(["hook", "--help"]));
    assert!(String::from_utf8(help.stdout).unwrap().contains("Usage:"));
    let bad = output(
        Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"))
            .args(["hook", "unknown-format"])
            .stdin(Stdio::null())
            .env("HOME", root.path())
            .env("TMUX", "/tmp/tsx-no-server,1,0")
            .env("TMUX_PANE", "%0")
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state")),
    );
    assert!(bad.status.success());
    assert!(bad.stdout.is_empty());
    assert!(bad.stderr.is_empty());
    assert!(
        fs::read_to_string(root.path().join("state/hook.log"))
            .unwrap()
            .contains("unknown hook format")
    );
}
