use serde_json::json;
use tmux_sidecar::{
    hook::signal::Signal,
    model::{AgentRun, AgentState},
    store::Store,
};
fn run() -> AgentRun {
    serde_json::from_value(json!({"id":"","server_start":1,"pane_verified":true,"pane_id":"%0","agent":"claude","agent_pid":42,"agent_session_id":null,"cwd":"/work","project_root":null,"started_at":1000,"ended_at":null,"state":"waiting","state_since":1000,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null})).unwrap()
}
fn signal(event: &str, at: u64) -> Signal {
    serde_json::from_value(json!({"server_start":1,"pane_verified":true,"pane_id":"%0","agent":"claude","agent_pid":42,"at":at,"nonce":format!("{at:032x}"),"source":"hook:claude","event":event,"cwd":"/work","payload":{}})).unwrap()
}
fn reading(status: &str, at: u64) -> session_board::model::Session {
    serde_json::from_value(json!({"id":"claude_code:known","agent":"claude_code","cwd":"/work","project_root":"/work","title":"known","started_at":1,"last_activity_at":at,"status":status,"last_user_text":"","last_assistant_text":"answer","transcript_path":"/work/known.jsonl","resume_hint":"","pane":null})).unwrap()
}
#[test]
fn notify_only_and_process_only_are_not_actionable() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut r = run();
    r.agent = tmux_sidecar::model::AgentKind::Codex;
    let id = store.insert(&r).unwrap();
    assert!(
        store.attention().unwrap().is_empty(),
        "process-only attention"
    );
    let mut s = signal("agent-turn-complete", 2000);
    s.agent = r.agent;
    s.source = "hook:codex-notify".into();
    store.signal(&s).unwrap();
    assert!(
        !store.has_state_evidence(&id).unwrap(),
        "notify is not send evidence"
    );
    assert!(
        store.attention().unwrap().is_empty(),
        "notify-only attention"
    );
    s.event = "PermissionRequest".into();
    s.at = 3000;
    s.nonce = format!("{:032x}", 3000);
    s.source = "hook:codex".into();
    store.signal(&s).unwrap();
    assert!(store.has_state_evidence(&id).unwrap());
    assert_eq!(store.attention().unwrap().len(), 1);
}
#[test]
fn orphan_session_end_cannot_end_a_live_pane_run() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&signal("Stop", 2000)).unwrap();
    let mut s = signal("SessionEnd", 3000);
    s.orphaned = true;
    s.pane_verified = false;
    s.agent_pid = 999;
    s.parent_agent_pid = Some(42);
    store.signal(&s).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Waiting);
    s.orphaned = false;
    s.pane_verified = true;
    s.agent_pid = 42;
    s.at = 4000;
    s.nonce = format!("{:032x}", 4000);
    store.signal(&s).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Done);
}
#[test]
fn interrupted_claude_transcript_can_clear_only_a_newer_block() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&signal("PermissionRequest", 2000)).unwrap();
    let id = store.active().unwrap()[0].id.clone();
    store.transcript(&id, &reading("waiting", 1), 3000).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Blocked);
    store.transcript(&id, &reading("working", 3), 3000).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Blocked);
    store.transcript(&id, &reading("idle", 3), 3000).unwrap();
    assert_eq!(
        store.active().unwrap()[0].state,
        AgentState::Blocked,
        "idle alone is not interruption evidence"
    );
    let mut interrupted = reading("idle", 3);
    interrupted.last_user_text = "[Request interrupted by user for tool use]".into();
    store.transcript(&id, &interrupted, 3000).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Waiting);
    store.signal(&signal("PermissionRequest", 4000)).unwrap();
    let mut interrupted = reading("waiting", 5);
    interrupted.last_user_text = "[Request interrupted by user]".into();
    store.transcript(&id, &interrupted, 5000).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Waiting);
}
#[test]
fn transcript_completion_is_waiting_and_legacy_done_can_resume() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut r = run();
    r.state = AgentState::Working;
    let id = store.insert(&r).unwrap();
    store.transcript(&id, &reading("done", 2), 2000).unwrap();
    assert_eq!(store.active().unwrap()[0].state, AgentState::Waiting);
    r.agent_pid = 43;
    r.pane_id = "%1".into();
    r.state = AgentState::Done;
    r.state_source = "transcript".into();
    let id = store.insert(&r).unwrap();
    store.transcript(&id, &reading("working", 3), 3000).unwrap();
    assert_eq!(
        store.get(id.parse().unwrap(), 1).unwrap().unwrap().state,
        AgentState::Working
    );
    store.end(&id, 4000).unwrap();
    store.transcript(&id, &reading("waiting", 5), 5000).unwrap();
    assert_eq!(
        store.get(id.parse().unwrap(), 1).unwrap().unwrap().state,
        AgentState::Done
    );
}
