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
pub fn command(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
    cmd.env("HOME", home)
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

pub fn run(cmd: &mut Command) -> std::process::Output {
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
