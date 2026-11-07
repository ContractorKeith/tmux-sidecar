use serde_json::json;
use tmux_sidecar::{
    hook::signal::Signal,
    model::{AgentRun, AgentState},
    store::Store,
};
#[test]
fn already_orphaned_session_start_binds_the_unique_unidentified_cwd_owner() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let run:AgentRun=serde_json::from_value(json!({"id":"","server_start":1,"pane_id":"%0","agent":"opencode","agent_pid":42,"agent_session_id":null,"cwd":"/work","project_root":null,"started_at":100,"ended_at":null,"state":"waiting","state_since":100,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null})).unwrap();
    let id = store.insert(&run).unwrap();
    let orphan:Signal=serde_json::from_value(json!({"server_start":1,"pane_id":"%99","agent":"opencode","agent_pid":999,"at":200,"nonce":format!("{:032x}",200),"source":"hook:opencode","event":"permission.asked","agent_session_id":"new-session","cwd":"/work","orphaned":true,"payload":{}})).unwrap();
    store.signal(&orphan).unwrap();
    let current = store.get(id.parse().unwrap(), 1).unwrap().unwrap();
    assert_eq!(current.state, AgentState::Blocked);
    assert_eq!(current.agent_session_id.as_deref(), Some("new-session"));
    assert_eq!(store.list().unwrap().len(), 1);
    let history = store.signal_history().unwrap();
    assert_eq!(history[0]["payload_json"], "{}");
}

#[test]
fn parent_client_identity_beats_ambiguous_unidentified_cwd_matches() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut run:AgentRun=serde_json::from_value(json!({"id":"","server_start":1,"pane_id":"%0","agent":"opencode","agent_pid":42,"agent_session_id":null,"cwd":"/work","project_root":null,"started_at":100,"ended_at":null,"state":"waiting","state_since":100,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null})).unwrap();
    let first = store.insert(&run).unwrap();
    run.agent_pid = 43;
    run.pane_id = "%1".into();
    store.insert(&run).unwrap();
    let mut orphan:Signal=serde_json::from_value(json!({"server_start":1,"pane_id":"%99","agent":"opencode","agent_pid":999,"at":200,"nonce":format!("{:032x}",200),"source":"hook:opencode","event":"permission.asked","agent_session_id":"new-session","cwd":"/work","orphaned":true,"payload":{}})).unwrap();
    store.signal(&orphan).unwrap();
    assert!(
        store
            .list()
            .unwrap()
            .iter()
            .all(|r| r.state == AgentState::Waiting)
    );
    assert!(store.signal_history().unwrap().is_empty());
    orphan.parent_agent_pid = Some(42);
    orphan.nonce = format!("{:032x}", 201);
    store.signal(&orphan).unwrap();
    assert_eq!(
        store.get(first.parse().unwrap(), 1).unwrap().unwrap().state,
        AgentState::Blocked
    );
    assert_eq!(store.list().unwrap().len(), 2);
}
