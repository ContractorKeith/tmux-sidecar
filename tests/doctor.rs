//! Complete doctor and hello behavior against a fake tmux, stores and socket peer.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
    process::Command,
    time::Duration,
};

fn fixture(home: &Path, binary: &Path) {
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
    for (name, script) in [
        ("tmux", "#!/bin/sh\necho 'tmux next-3.6'\n"),
        ("notify-send", "#!/bin/sh\nexit 0\n"),
        ("osascript", "#!/bin/sh\nexit 0\n"),
    ] {
        let path = home.join("bin").join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
fn command(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
    cmd.env("HOME", home)
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
fn peer(home: &Path, version: &str, count: usize) -> std::thread::JoinHandle<()> {
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
                            "daemon_version":version, "server_key":key, "rpc_version":1}});
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
#[test]
fn doctor_on_complete_and_missing_fakes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")));
    let server = peer(root.path(), env!("CARGO_PKG_VERSION"), 3);
    use std::process::Stdio;
    // kill_on_drop-like ownership plus timeout is supplied by the shared test helper below.
    fn run(cmd: &mut Command) -> std::process::Output {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
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
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while guard.0.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline, "doctor deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
        guard.0.take().unwrap().wait_with_output().unwrap()
    }
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::remove_file(root.path().join("bin/tmux")).unwrap();
    fs::remove_file(root.path().join("bin/notify-send")).unwrap();
    fs::remove_file(root.path().join("bin/osascript")).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    for name in ["tmux", "notifier"] {
        assert_eq!(
            checks.iter().find(|c| c["name"] == name).unwrap()["ok"],
            false
        );
    }
    // Insecure runtime must be a reported check, not prevent all output.
    fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o755)).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks
            .iter()
            .find(|c| c["name"] == "runtime permissions")
            .unwrap()["ok"],
        false
    );
    server.join().unwrap();
    fs::remove_file(root.path().join("run/sock")).unwrap();
    fs::set_permissions(root.path().join("run"), fs::Permissions::from_mode(0o700)).unwrap();
    let skew = peer(root.path(), "0.0.0", 2);
    let output = run(command(root.path()).args(["doctor", "--json"]));
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.matches("differs from client").count(), 1);
    assert!(stderr.contains("daemon stop"));
    let output = run(command(root.path()).args(["daemon", "status", "--json"]));
    assert!(
        output.status.success(),
        "version skew must not fail ordinary clients"
    );
    assert_eq!(
        String::from_utf8(output.stderr)
            .unwrap()
            .matches("differs from client")
            .count(),
        1
    );
    skew.join().unwrap();
    fs::remove_file(root.path().join("run/sock")).unwrap();
    let output = run(command(root.path()).args(["doctor", "--json"]));
    let checks: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        checks.iter().find(|c| c["name"] == "daemon").unwrap()["ok"],
        false
    );
    assert!(!root.path().join("run/daemon.pid").exists());
}
