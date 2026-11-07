use serde_json::{Value, json};
use tmux_sidecar::{daemon, hook::signal::Signal, model::AgentKind, rpc, store::Store};
fn signal(pid: u32, event: &str, at: u64) -> Signal {
    Signal {
        server_start: 1,
        pane_id: format!("%{pid}"),
        agent: AgentKind::Claude,
        agent_pid: pid,
        at,
        source: "hook:claude".into(),
        nonce: format!("{pid:016x}{at:016x}"),
        event: event.into(),
        detail: None,
        agent_session_id: None,
        cwd: "/work".into(),
        transcript_path: None,
        payload: json!({}),
    }
}
fn call(store: &Store, method: &str, params: Value) -> Value {
    let response = daemon::dispatch(
        rpc::Request {
            jsonrpc: rpc::Version::V2,
            id: rpc::Id::Number(1),
            method: method.into(),
            params: Some(params),
        },
        store,
        "test",
        1000,
    );
    match response.outcome {
        rpc::Outcome::Success { result } => result,
        _ => panic!("{response:?}"),
    }
}
#[test]
fn attention_priority_age_read_changes_and_late_signal_history() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    for (pid, event, at) in [
        (1, "UserPromptSubmit", 100),
        (2, "StopFailure", 200),
        (3, "PermissionRequest", 300),
        (4, "PermissionRequest", 150),
    ] {
        store.signal(&signal(pid, event, at)).unwrap();
    }
    store.signal(&signal(1, "Stop", 110)).unwrap();
    let queue = call(&store, "attention", json!({}));
    assert_eq!(
        queue
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["agent_pid"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![4, 3, 2, 1]
    );
    let id = queue[0]["id"].as_str().unwrap();
    call(&store, "ack", json!({"sel":id}));
    assert_eq!(
        call(&store, "attention", json!({}))
            .as_array()
            .unwrap()
            .len(),
        3
    );
    store.signal(&signal(4, "UserPromptSubmit", 400)).unwrap();
    store.signal(&signal(4, "PermissionRequest", 500)).unwrap();
    store.signal(&signal(4, "Stop", 450)).unwrap();
    let run = store.get(id.parse().unwrap(), 1).unwrap().unwrap();
    assert_eq!(run.state, tmux_sidecar::model::AgentState::Blocked);
    assert_eq!(run.state_since, 500);
    assert_eq!(run.read_at, None);
    assert!(store.has_hooks(id).unwrap());
    let before = call(&store, "history", json!({"sel":id}));
    store.signal(&signal(4, "PermissionRequest", 500)).unwrap();
    assert_eq!(call(&store, "history", json!({"sel":id})), before);
    let history = call(&store, "history", json!({"sel":id,"limit":2}));
    assert_eq!(history.as_array().unwrap().len(), 2);
    assert_eq!(history[0]["event"], "state_changed");
    assert!(history[0]["payload"]["cause"].is_number());
    assert_eq!(history[0]["payload"]["pane_id"], "%4");
    assert_eq!(history[0]["payload"]["server_start"], 1);
    store.end(id, 600).unwrap();
    store.signal(&signal(4, "PreToolUse", 700)).unwrap();
    assert_eq!(
        store.get(id.parse().unwrap(), 1).unwrap().unwrap().state,
        tmux_sidecar::model::AgentState::Done
    );
}
