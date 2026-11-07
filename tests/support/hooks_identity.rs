use super::{TmuxGuard, enabled};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::Path,
    time::{Duration, Instant},
};
e2e_test!(
    hook_verifies_server_key_and_spools_instead_of_cross_delivery,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let runs = super::lifecycle::list(&guard);
        let pid = runs[0]["agent_pid"].as_u64().unwrap();
        super::lifecycle::stop(&guard);
        let lock = tmux_sidecar::client::FileLock::try_acquire(
            &guard.scratch.path().join("run/daemon.lock"),
        )
        .unwrap();
        let listener = UnixListener::bind(guard.scratch.path().join("run/sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            // macOS inherits O_NONBLOCK from the listener; Linux does not.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            assert!(line.contains("hello"));
            stream
                .get_mut()
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"server_key\":\"wrong\"}}\n",
                )
                .unwrap();
            line.clear();
            assert_eq!(
                stream.read_line(&mut line).unwrap(),
                0,
                "cross-server signal: {line}"
            );
        });
        super::hooks::fire(
            &guard,
            pid,
            "claude",
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude/Stop.json"),
        );
        server.join().unwrap();
        let spool = fs::read_to_string(guard.scratch.path().join("state/spool.ndjson")).unwrap();
        assert!(spool.contains("Stop"));
        drop(lock);
    }
);
e2e_test!(client_joins_an_already_starting_daemon, {
    if !enabled() {
        return;
    }
    use std::{os::unix::fs::PermissionsExt, process::Stdio};
    let guard = TmuxGuard::start();
    let ps = guard.scratch.path().join("bin/ps");
    fs::write(
        &ps,
        b"#!/bin/sh\ntouch \"$HOME/starting\"\nsleep 0.4\nexec /bin/ps \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let _daemon = super::support::ChildGuard::new(
        guard
            .sidecar()
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| guard.scratch.path().join("home/starting").exists());
    assert_eq!(super::lifecycle::list(&guard).as_array().unwrap().len(), 1);
    super::lifecycle::stop(&guard);
});
e2e_test!(shutdown_drains_already_written_signal_notifications, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let runs = super::lifecycle::list(&guard);
    let run = &runs[0];
    let mut stream =
        crate::support::socket::connect(guard.scratch.path().join("run/sock")).unwrap();
    for at in 1..=200 {
        let frame = serde_json::json!({"jsonrpc":"2.0","method":"signal","params":{
            "server_start":run["server_start"],"pane_id":run["pane_id"],"agent":run["agent"],"agent_pid":run["agent_pid"],
            "at":at,"nonce":format!("{at:032x}"),"source":"hook:claude","event":"Stop","detail":null,"agent_session_id":null,"transcript_path":null,"cwd":"/fixture","payload":{"hook_event_name":"Stop"}
        }});
        writeln!(stream, "{frame}").unwrap();
    }
    super::lifecycle::stop(&guard);
    drop(stream);
    let store =
        tmux_sidecar::store::Store::open(&guard.scratch.path().join("state/tmux-sidecar.db"))
            .unwrap();
    assert_eq!(store.signal_history().unwrap().len(), 200);
});
