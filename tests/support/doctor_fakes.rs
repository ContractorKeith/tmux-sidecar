//! Complete doctor and hello behavior against a fake tmux, stores and socket peer.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

static CACHE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
extern "C" fn cleanup() {
    if let Some(root) = CACHE.get() {
        let _ = fs::remove_dir_all(root.path());
    }
}
fn cache() -> &'static Path {
    CACHE
        .get_or_init(|| {
            let root = tempfile::Builder::new()
                .prefix("tsx-doctor-")
                .tempdir_in("/tmp")
                .unwrap();
            for name in ["tmux", "notify-send", "osascript", "claude"] {
                let path = root.path().join(name);
                let script = if name == "tmux" {
                    "#!/bin/sh\necho 'tmux next-3.6'\n"
                } else {
                    "#!/bin/sh\nexit 0\n"
                };
                fs::write(&path, script).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
                // Warm each executable inode before any diagnostic deadline starts.
                assert!(
                    run(Command::new(&path).stdin(Stdio::null()))
                        .status
                        .success()
                );
            }
            // SAFETY: callback has no arguments and remains valid for this process.
            assert_eq!(unsafe { libc::atexit(cleanup) }, 0);
            root
        })
        .path()
}
pub fn fixture(home: &Path, binary: &Path) {
    tmux_sidecar::install::run(home, binary, &[], true, false, false).unwrap();
    for dir in [
        ".claude/projects",
        ".codex/sessions",
        ".grok/sessions",
        "bin",
        "run",
        "state",
    ] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
    for dir in ["run", "state"] {
        fs::set_permissions(home.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    for file in [".grok/sessions/session_search.sqlite", "opencode.db"] {
        fs::write(home.join(file), "fake read-only store").unwrap();
    }
    for name in ["tmux", "notify-send", "osascript", "claude"] {
        fs::hard_link(cache().join(name), home.join("bin").join(name)).unwrap();
    }
}
pub fn command(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
    cmd.env("HOME", home)
        .env(
            "TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER",
            std::env::var("TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER").unwrap_or_else(|_| "1".into()),
        )
        .env("TMUX_TMPDIR", home)
        .env("PATH", home.join("bin"))
        .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
        .env("CODEX_HOME", home.join(".codex"))
        .env("GROK_HOME", home.join(".grok"))
        .env("OPENCODE_DB", home.join("opencode.db"))
        .env("TMUX_SIDECAR_RUNTIME_DIR", home.join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", home.join("state"))
        .arg("--tmux-socket")
        .arg(home.join("tmux"));
    cmd
}
// Thread owns no subprocess and both accept and socket reads have deadlines.
pub fn peer(home: &Path, version: &str, count: usize) -> std::thread::JoinHandle<()> {
    let listener = UnixListener::bind(home.join("run/sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let key = tmux_sidecar::paths::server_key(&home.join("tmux")).unwrap();
    let version = version.to_owned();
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut served = 0;
        while served < count {
            match listener.accept() {
                Ok((socket, _)) => {
                    // macOS inherits O_NONBLOCK from the listener; Linux does not.
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut socket = BufReader::new(socket);
                    loop {
                        let mut line = String::new();
                        if socket.read_line(&mut line).unwrap() == 0 {
                            break;
                        }
                        let request: Value = serde_json::from_str(&line).unwrap();
                        let reply = json!({"jsonrpc":"2.0", "id":request["id"], "result":{
                            "daemon_version":version, "build_id":env!("TMUX_SIDECAR_BUILD_ID"), "server_key":key, "rpc_version":1}});
                        writeln!(socket.get_mut(), "{reply}").unwrap();
                    }
                    served += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "fake peer deadline");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        }
    })
}

pub fn run(cmd: &mut Command) -> std::process::Output {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().unwrap();
    struct Guard(Option<std::process::Child>);
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut guard = Guard(Some(child));
    let deadline = std::time::Instant::now()
        + Duration::from_secs(5)
            * std::env::var("TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(1)
                .clamp(1, 4);
    while guard.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline, "doctor deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
    guard.0.take().unwrap().wait_with_output().unwrap()
}
