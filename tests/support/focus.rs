use super::lifecycle::{list, stop};
use super::state::rpc;
use super::{TmuxGuard, enabled};
use crate::support::{ChildGuard, output};
use serde_json::json;
use std::process::Stdio;
e2e_test!(focus_with_two_attached_clients_and_detached_session, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    for name in ["second", "detached"] {
        assert!(
            output(
                guard
                    .command()
                    .args(["new-session", "-d", "-s", name])
                    .arg(guard.scratch.path().join("bin/claude"))
            )
            .status
            .success()
        );
    }
    assert!(
        output(
            guard
                .command()
                .args(["new-window", "-d", "-t", "fixture"])
                .arg(guard.scratch.path().join("bin/claude"))
        )
        .status
        .success()
    );
    until!(|| list(&guard).as_array().unwrap().len() == 4);
    let mut clients = Vec::new();
    for name in ["fixture", "second"] {
        clients.push(ChildGuard::new(
            guard
                .command()
                .args(["-C", "attach-session", "-t", name])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        ));
    }
    until!(|| {
        use tmux_sidecar::seams::Tmux;
        guard
            .adapter
            .list_panes()
            .unwrap()
            .iter()
            .filter(|p| p.session_attached)
            .count()
            == 3
    });
    for run in list(&guard).as_array().unwrap() {
        rpc(
            &guard,
            "signal",
            json!({"server_start":run["server_start"],"pane_id":run["pane_id"],"agent":"claude","agent_pid":run["agent_pid"],"at":tmux_sidecar::daemon::now(),"source":"hook:claude","nonce":format!("{:032x}",run["agent_pid"].as_u64().unwrap()),"event":"PermissionRequest","cwd":run["cwd"],"payload":{}}),
        );
    }
    until!(|| list(&guard)
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["read_at"].is_number())
        .count()
        == 2);
    let queue = rpc(&guard, "attention", json!({}));
    assert_eq!(queue.as_array().unwrap().len(), 2);
    use tmux_sidecar::seams::Tmux;
    let panes = guard.adapter.list_panes().unwrap();
    for run in list(&guard).as_array().unwrap() {
        let pane = panes.iter().find(|p| p.pane_id == run["pane_id"]).unwrap();
        assert_eq!(
            run["read_at"].is_number(),
            pane.pane_active && pane.window_active && pane.session_attached
        );
    }
    // Switching to the previously hidden window reads that run on the next tick.
    assert!(
        output(guard.command().args(["select-window", "-t", "fixture:1"]))
            .status
            .success()
    );
    until!(|| rpc(&guard, "attention", json!({}))
        .as_array()
        .unwrap()
        .len()
        == 1);
    drop(clients);
    stop(&guard);
});
