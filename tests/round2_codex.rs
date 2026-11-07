use serde_json::json;
use tmux_sidecar::{hook::signal::Signal, model::AgentState, store::Store};
fn signal(source: &str, event: &str, session: Option<&str>, at: u64) -> Signal {
    serde_json::from_value(json!({"server_start":1,"pane_verified":true,"pane_id":"%0","agent":"codex","agent_pid":42,"at":at,"nonce":format!("{at:032x}"),"source":source,"event":event,"agent_session_id":session,"cwd":"/work","payload":{"input-messages":["Create a short label for this task"]}})).unwrap()
}
#[test]
fn changed_title_prompt_cannot_poison_notify_only_or_later_inline_identity() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store
        .signal(&signal(
            "hook:codex-notify",
            "agent-turn-complete",
            Some("title-thread"),
            100,
        ))
        .unwrap();
    assert!(store.list().unwrap()[0].agent_session_id.is_none());
    store
        .signal(&signal("hook:codex", "UserPromptSubmit", None, 200))
        .unwrap();
    store
        .signal(&signal(
            "hook:codex-notify",
            "agent-turn-complete",
            Some("real-thread"),
            300,
        ))
        .unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Waiting);
    assert!(store.list().unwrap()[0].agent_session_id.is_none());
    store
        .signal(&signal(
            "hook:codex",
            "UserPromptSubmit",
            Some("real-thread"),
            400,
        ))
        .unwrap();
    store
        .signal(&signal(
            "hook:codex-notify",
            "agent-turn-complete",
            Some("title-thread"),
            500,
        ))
        .unwrap();
    let run = store.list().unwrap().remove(0);
    assert_eq!(run.agent_session_id.as_deref(), Some("real-thread"));
    assert_eq!(run.state, AgentState::Working);
    store
        .signal(&signal(
            "hook:codex-notify",
            "agent-turn-complete",
            Some("real-thread"),
            600,
        ))
        .unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Waiting);
}
