use super::{
    TmuxGuard, enabled,
    lifecycle::{list, stop},
    support::{ChildGuard, output},
};
use std::{
    io::{BufRead, BufReader},
    os::unix::process::CommandExt,
    process::Stdio,
};

e2e_test!(connection_cap_rejects_extras_and_recovers, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    let socket = g.scratch.path().join("run/sock");
    let mut clients = Vec::new();
    for _ in 0..256 {
        clients.push(crate::support::socket::connect(&socket).unwrap());
    }
    let extra = crate::support::socket::connect(&socket).unwrap();
    extra
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut frame = String::new();
    BufReader::new(extra).read_line(&mut frame).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&frame).unwrap()["error"]["code"],
        -32600
    );
    drop(clients);
    until!(|| output(g.sidecar().args(["daemon", "status", "--json"]))
        .status
        .success());
    assert_eq!(list(&g).as_array().unwrap().len(), 1);
});

e2e_test!(emfile_accept_error_keeps_daemon_alive_and_recovers, {
    if !enabled() {
        return;
    }
    let g = TmuxGuard::start();
    list(&g);
    stop(&g);
    let mut command = g.sidecar();
    command
        .args(["daemon", "run"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: pre_exec only calls async-signal-safe setrlimit with initialized data.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 96,
                rlim_max: 96,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut daemon = ChildGuard::new(command.spawn().unwrap());
    until!(|| g.scratch.path().join("run/sock").exists());
    let socket = g.scratch.path().join("run/sock");
    let mut clients = Vec::new();
    // The shared connector bounds backlog waits and keeps every descriptor CLOEXEC.
    for _ in 0..180 {
        match crate::support::socket::connect(&socket) {
            Ok(stream) => clients.push(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(error) => panic!("connect pressure client: {error}"),
        }
    }
    until!(
        || std::fs::read_to_string(g.scratch.path().join("state/daemon.log"))
            .unwrap_or_default()
            .contains("accept failed")
    );
    let hold = std::time::Instant::now() + std::time::Duration::from_millis(1200);
    while std::time::Instant::now() < hold {
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "EMFILE must not consume the poll failure budget"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(clients);
    until!(|| output(g.sidecar().args(["daemon", "status", "--json"]))
        .status
        .success());
    assert!(daemon.0.try_wait().unwrap().is_none());
    stop(&g);
});
