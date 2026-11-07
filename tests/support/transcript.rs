use super::lifecycle::{list, stop};
use super::state::rpc;
use super::{TmuxGuard, enabled};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn fixture(guard: &TmuxGuard, cwd: &str) -> std::path::PathBuf {
    let dir = guard.scratch.path().join("home/.claude/projects/fixture");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.jsonl");
    let timestamp = String::from_utf8(
        crate::support::output(
            std::process::Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        )
        .stdout,
    )
    .unwrap();
    let record = json!({"type":"user","sessionId":"fixture","cwd":cwd,"timestamp":timestamp.trim(),"message":{"role":"user","content":"build this"}});
    fs::write(&path, format!("{record}\n")).unwrap();
    path
}
e2e_test!(
    transcript_fallback_then_hook_payload_owns_state_and_metadata,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let run = list(&guard)[0].clone();
        let path = fixture(&guard, run["cwd"].as_str().unwrap());
        until!(|| list(&guard)[0]["state"] == "working");
        assert_eq!(list(&guard)[0]["transcript_path"], path.to_str().unwrap());
        let stop_fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks/claude/Stop.json");
        super::hooks::fire(
            &guard,
            run["agent_pid"].as_u64().unwrap(),
            "claude",
            &stop_fixture,
        );
        until!(|| list(&guard)[0]["state"] == "waiting");
        // Once hooked, only subsequent hook payloads update state and metadata.
        let mut record: Value =
            serde_json::from_str(fs::read_to_string(&path).unwrap().trim()).unwrap();
        record["type"] = json!("assistant");
        record["message"] = json!({"role":"assistant","content":[{"type":"text","text":"fixture response"}],"stop_reason":"end_turn"});
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new().append(true).open(&path).unwrap(),
            "{record}"
        )
        .unwrap();
        // Fixture Stop carries a different session id, so bind the matching transcript explicitly.
        let mut signal =
            serde_json::from_slice::<Value>(&fs::read(&stop_fixture).unwrap()).unwrap();
        signal["session_id"] = json!("fixture");
        signal["transcript_path"] = json!(path);
        signal["last_assistant_message"] = json!("fixture response");
        let updated = guard.scratch.path().join("stop.json");
        fs::write(&updated, signal.to_string()).unwrap();
        super::hooks::fire(
            &guard,
            run["agent_pid"].as_u64().unwrap(),
            "claude",
            &updated,
        );
        until!(|| list(&guard)[0]["last_assistant_text"] == "fixture response");
        assert_eq!(list(&guard)[0]["state"], "waiting");
        stop(&guard);
    }
);
e2e_test!(project_groups_and_manual_membership_survive_restart, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let repo = guard.scratch.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    assert!(
        crate::support::output(std::process::Command::new("git").arg("init").arg(&repo))
            .status
            .success()
    );
    for _ in 0..2 {
        assert!(
            crate::support::output(
                guard
                    .command()
                    .args(["new-window", "-d", "-c"])
                    .arg(&repo)
                    .arg(guard.scratch.path().join("bin/claude"))
            )
            .status
            .success()
        );
    }
    until!(|| list(&guard).as_array().unwrap().len() == 3);
    let grouped = rpc(&guard, "list", json!({"group":repo}));
    assert_eq!(grouped.as_array().unwrap().len(), 2);
    let selected = grouped[0].clone();
    rpc(
        &guard,
        "group.add",
        json!({"group":"review","selectors":[selected["id"]]}),
    );
    assert_eq!(
        rpc(&guard, "list", json!({"group":"review"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let pane = selected["pane_id"].as_str().unwrap();
    rpc(&guard, "name", json!({"sel":pane,"name":"reviewer"}));
    assert!(
        crate::support::output(
            guard
                .command()
                .args(["respawn-pane", "-k", "-t", pane, "-c"])
                .arg(&repo)
                .arg(guard.scratch.path().join("bin/claude"))
        )
        .status
        .success()
    );
    until!(|| {
        let runs = rpc(&guard, "list", json!({"group":"review"}));
        runs.as_array().unwrap().len() == 1 && runs[0]["id"] != selected["id"]
    });
    rpc(
        &guard,
        "group.rm",
        json!({"group":"review","selectors":[pane]}),
    );
    assert_eq!(rpc(&guard, "list", json!({"group":"review"})), json!([]));
    stop(&guard);
    let store =
        tmux_sidecar::store::Store::open(&guard.scratch.path().join("state/tmux-sidecar.db"))
            .unwrap();
    assert_eq!(
        store
            .pane_name(selected["server_start"].as_u64().unwrap(), pane)
            .unwrap()
            .as_deref(),
        Some("reviewer")
    );
});
