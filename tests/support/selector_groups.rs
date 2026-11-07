use super::lifecycle::{list, stop};
use super::state::rpc;
use super::{TmuxGuard, enabled};
use crate::support::{ChildGuard, output};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, process::Stdio};
e2e_test!(pane_selector_uses_cached_server_identity_not_newest_run, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let ps = guard.scratch.path().join("bin/ps");
    fs::write(&ps,"#!/bin/sh\nn=0\n[ ! -f \"$HOME/count\" ] || n=$(cat \"$HOME/count\")\nn=$((n+1))\necho $n > \"$HOME/count\"\nif [ $n -gt 1 ]; then touch \"$HOME/slow\"; sleep 3; fi\nexec /bin/ps \"$@\"\n").unwrap();
    fs::set_permissions(ps, fs::Permissions::from_mode(0o700)).unwrap();
    let _daemon = ChildGuard::new(
        guard
            .sidecar()
            .args(["daemon", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    until!(|| guard.scratch.path().join("home/slow").exists());
    let run = list(&guard)[0].clone();
    rpc(
        &guard,
        "signal",
        json!({"server_start":run["server_start"].as_u64().unwrap()+1,"pane_id":run["pane_id"],"agent":"claude","agent_pid":run["agent_pid"],"at":tmux_sidecar::daemon::now(),"nonce":"00000000000000000000000000000099","source":"hook:claude","event":"PermissionRequest","cwd":run["cwd"],"payload":{}}),
    );
    rpc(&guard, "ack", json!({"sel":run["pane_id"]}));
    let runs = list(&guard);
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == run["id"])
            .unwrap()["read_at"]
            .is_number()
    );
    assert!(
        runs.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] != run["id"])
            .unwrap()["read_at"]
            .is_null()
    );
    stop(&guard);
});
e2e_test!(manual_and_project_groups_have_distinct_namespaces, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let project = tempfile::Builder::new()
        .prefix("tsx-group-")
        .tempdir_in(guard.scratch.path())
        .unwrap();
    let repo = project.path().canonicalize().unwrap();
    assert!(
        output(std::process::Command::new("git").arg("init").arg(&repo))
            .status
            .success()
    );
    assert!(
        output(
            guard
                .command()
                .args(["new-window", "-d", "-c"])
                .arg(&repo)
                .arg(guard.scratch.path().join("bin/claude"))
        )
        .status
        .success()
    );
    until!(|| list(&guard).as_array().unwrap().len() == 2);
    let outside = list(&guard)
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pane_id"] == "%0")
        .unwrap()
        .clone();
    rpc(
        &guard,
        "group.add",
        json!({"group":repo,"sel":outside["id"]}),
    );
    let manual = rpc(
        &guard,
        "list",
        json!({"group":format!("manual:{}",repo.display())}),
    );
    assert_eq!(manual.as_array().unwrap().len(), 1);
    assert_eq!(manual[0]["id"], outside["id"]);
    let project = rpc(
        &guard,
        "list",
        json!({"group":format!("project:{}",repo.display())}),
    );
    assert_eq!(project.as_array().unwrap().len(), 1);
    assert_ne!(project[0]["id"], outside["id"]);
    assert_eq!(rpc(&guard, "list", json!({"group":repo})), project);
    stop(&guard);
});
