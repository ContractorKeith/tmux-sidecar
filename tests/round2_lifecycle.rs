use serde_json::json;
use tmux_sidecar::{hook::signal::Signal, model::AgentState, store::Store};
fn hook(event: &str, session: &str, at: u64) -> Signal {
    serde_json::from_value(json!({"server_start":1,"pane_verified":true,"pane_id":"%0","agent":"claude","agent_pid":42,"at":at,"nonce":format!("{at:032x}"),"source":"hook:claude","event":event,"agent_session_id":session,"cwd":"/work","payload":{}})).unwrap()
}
#[test]
fn clear_and_logout_leave_a_live_process_ready_for_its_next_session() {
    for reason in ["clear", "logout"] {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("db")).unwrap();
        store.signal(&hook("UserPromptSubmit", "old", 100)).unwrap();
        let mut clear = hook("SessionEnd", "old", 200);
        clear.detail = Some(reason.into());
        store.signal(&clear).unwrap();
        assert_eq!(store.list().unwrap()[0].state, AgentState::Waiting);
        store.signal(&hook("UserPromptSubmit", "new", 300)).unwrap();
        let run = store.list().unwrap().remove(0);
        assert_eq!(run.state, AgentState::Working);
        assert_eq!(run.agent_session_id.as_deref(), Some("new"));
        assert!(run.ended_at.is_none());
    }
}
#[test]
fn hook_done_allows_new_sessions_and_prompts_but_process_exit_is_final() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut end = hook("SessionEnd", "old", 100);
    end.agent = tmux_sidecar::model::AgentKind::Grok;
    store.signal(&end).unwrap();
    let mut stop = end.clone();
    stop.event = "Stop".into();
    stop.at = 102;
    stop.nonce = format!("{:032x}", 102);
    store.signal(&stop).unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Done);
    let mut next = hook("PreToolUse", "new", 200);
    next.agent = end.agent;
    store.signal(&next).unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Working);
    end.at = 300;
    end.nonce = format!("{:032x}", 300);
    end.agent_session_id = Some("new".into());
    store.signal(&end).unwrap();
    next.event = "UserPromptSubmit".into();
    next.at = 400;
    next.nonce = format!("{:032x}", 400);
    store.signal(&next).unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Working);
    end.at = 500;
    end.nonce = format!("{:032x}", 500);
    store.signal(&end).unwrap();
    let id = store.list().unwrap()[0].id.clone();
    store.end(&id, 600).unwrap();
    next.at = 700;
    next.nonce = format!("{:032x}", 700);
    next.agent_session_id = Some("after-exit".into());
    store.signal(&next).unwrap();
    let run = store.list().unwrap().remove(0);
    assert_eq!(run.state, AgentState::Done);
    assert_eq!(run.ended_at, Some(600));
}
