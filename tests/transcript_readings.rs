use serde_json::json;
use tmux_sidecar::{model::AgentRun, store::Store};
#[test]
fn unchanged_readings_do_not_flood_history_but_changes_are_kept() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut run: AgentRun = serde_json::from_value(json!({"id":"","server_start":1,"pane_id":"%0","agent":"claude","agent_pid":42,"agent_session_id":null,"cwd":root.path(),"project_root":null,"started_at":1000,"ended_at":null,"state":"waiting","state_since":1000,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null})).unwrap();
    let id = store.insert(&run).unwrap();
    run.id = id.clone();
    let mut reading = serde_json::from_value(json!({"id":"claude_code:known","agent":"claude_code","cwd":root.path(),"project_root":root.path(),"title":"known","started_at":1,"last_activity_at":3,"status":"waiting","last_user_text":"","last_assistant_text":"answer","transcript_path":root.path().join("known.jsonl"),"resume_hint":"","pane":null})).unwrap();
    store.transcript(&id, &reading, 3000).unwrap();
    store.transcript(&id, &reading, 5000).unwrap();
    assert_eq!(
        store.history(&run, 100).unwrap().as_array().unwrap().len(),
        1
    );
    reading.last_assistant_text = "updated answer".into();
    store.transcript(&id, &reading, 7000).unwrap();
    assert_eq!(
        store.history(&run, 100).unwrap().as_array().unwrap().len(),
        2
    );
    drop(store);
    let store = Store::open(&root.path().join("db")).unwrap();
    store.transcript(&id, &reading, 9000).unwrap();
    assert_eq!(
        store.history(&run, 100).unwrap().as_array().unwrap().len(),
        2
    );
}
