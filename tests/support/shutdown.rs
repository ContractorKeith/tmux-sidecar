use super::{
    enabled,
    lifecycle::{list, stop},
    support::TmuxGuard,
};
use std::{fs, os::unix::fs::PermissionsExt};

e2e_test!(slow_process_snapshot_does_not_block_rpc, {
    if !enabled() {
        return;
    }
    use std::{
        io::{BufRead, BufReader, Write},
        time::Duration,
    };
    let g = TmuxGuard::start();
    let ps = g.scratch.path().join("bin/ps");
    fs::write(&ps, b"#!/bin/sh\nn=0\n[ ! -f \"$HOME/count\" ] || n=$(cat \"$HOME/count\")\nn=$((n+1))\necho $n > \"$HOME/count\"\nif [ $n -gt 1 ]; then touch \"$HOME/slow\"; sleep 3; fi\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let mut daemon = super::support::ChildGuard::new(
        g.sidecar()
            .args(["daemon", "run"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| g.scratch.path().join("home/slow").exists());
    let mut stream = crate::support::socket::connect(g.scratch.path().join("run/sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    writeln!(
        stream,
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\"}}"
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .expect("read RPC response within socket deadline");
    let response: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["result"]["rpc_version"], 1);
    assert_eq!(list(&g).as_array().unwrap().len(), 1);
    stop(&g);
    assert!(
        daemon.0.try_wait().unwrap().is_some(),
        "daemon stop returned before the acknowledged process exited"
    );
});

e2e_test!(exited_child_is_not_running_before_reap, {
    let root = crate::support::Scratch::new();
    let mut child = super::support::ChildGuard::new(
        std::process::Command::new("/bin/sleep")
            .arg("0")
            .env("HOME", root.path())
            .env("TMUX_TMPDIR", root.path())
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"))
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id() as i32;
    until!(|| !tmux_sidecar::proc::running(pid).unwrap());
    assert!(
        tmux_sidecar::proc::alive(pid),
        "unreaped child should retain its zombie pid"
    );
    assert!(child.0.try_wait().unwrap().is_some());
    assert!(!tmux_sidecar::proc::running(pid).unwrap());
});
