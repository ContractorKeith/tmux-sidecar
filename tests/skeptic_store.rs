use serde_json::json;
use tmux_sidecar::{hook::signal::Signal, model::AgentKind, store::Store};
fn signal(pid: u32, event: &str, at: u64) -> Signal {
    Signal {
        server_key: None,
        pane_verified: true,
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
        orphaned: false,
        parent_agent_pid: None,
        payload: json!({}),
    }
}

#[test]
fn session_end_is_terminal_before_process_poll() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&signal(1, "SessionEnd", 100)).unwrap();
    store.signal(&signal(1, "Stop", 102)).unwrap();
    assert_eq!(
        store.list().unwrap()[0].state,
        tmux_sidecar::model::AgentState::Done
    );
}

#[test]
fn codex_auxiliary_notify_cannot_finish_the_main_turn() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut hook = signal(1, "UserPromptSubmit", 100);
    hook.agent = AgentKind::Codex;
    hook.source = "hook:codex".into();
    hook.agent_session_id = Some("main-thread".into());
    store.signal(&hook).unwrap();
    let mut notify = hook.clone();
    notify.at = 110;
    notify.nonce = format!("{:032x}", 110);
    notify.source = "hook:codex-notify".into();
    notify.event = "agent-turn-complete".into();
    notify.agent_session_id = Some("title-thread".into());
    notify.payload =
        serde_json::from_str(include_str!("fixtures/hooks/codex/title-notify.json")).unwrap();
    store.signal(&notify).unwrap();
    let run = &store.list().unwrap()[0];
    assert_eq!(run.state, tmux_sidecar::model::AgentState::Working);
    assert_eq!(run.agent_session_id.as_deref(), Some("main-thread"));
    // Before native identity is known, the title prompt still cannot finish work.
    hook.agent_pid = 2;
    hook.pane_id = "%2".into();
    hook.nonce = format!("{:032x}", 200);
    hook.agent_session_id = None;
    store.signal(&hook).unwrap();
    notify.agent_pid = 2;
    notify.pane_id = "%2".into();
    notify.nonce = format!("{:032x}", 210);
    store.signal(&notify).unwrap();
    assert_eq!(
        store.list().unwrap()[1].state,
        tmux_sidecar::model::AgentState::Working
    );
    assert!(store.list().unwrap()[1].agent_session_id.is_none());
    notify.agent_session_id = Some("main-thread".into());
    notify.payload = json!({"type":"agent-turn-complete","thread-id":"main-thread","input-messages":["Build the requested feature"]});
    notify.nonce = format!("{:032x}", 220);
    store.signal(&notify).unwrap();
    assert_eq!(
        store.list().unwrap()[1].state,
        tmux_sidecar::model::AgentState::Waiting
    );
}

#[test]
fn orphaned_hook_uses_session_owner_and_never_creates_a_stale_pane_run() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let mut owner = signal(1, "UserPromptSubmit", 100);
    owner.agent = AgentKind::Opencode;
    owner.agent_session_id = Some("owned".into());
    store.signal(&owner).unwrap();
    let mut orphan = owner;
    orphan.agent_pid = 999;
    orphan.pane_id = "%99".into();
    orphan.event = "permission.asked".into();
    orphan.at = 101;
    orphan.nonce = format!("{:032x}", 999);
    orphan.orphaned = true;
    store.signal(&orphan).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
    assert_eq!(
        store.list().unwrap()[0].state,
        tmux_sidecar::model::AgentState::Blocked
    );
    orphan.agent_session_id = Some("unowned".into());
    orphan.nonce = format!("{:032x}", 1000);
    store.signal(&orphan).unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn send_without_hook_confirmation_reverts_but_confirmed_send_stays_working() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&signal(1, "Stop", 100)).unwrap();
    let run = store.list().unwrap().remove(0);
    store.sent(&run, 200).unwrap();
    store.maintain(&[], 30_201, 1_800_000).unwrap();
    assert_eq!(
        store.list().unwrap()[0].state,
        tmux_sidecar::model::AgentState::Waiting
    );
    let run = store.list().unwrap().remove(0);
    store.sent(&run, 40_000).unwrap();
    store
        .signal(&signal(1, "UserPromptSubmit", 40_001))
        .unwrap();
    store.maintain(&[], 80_000, 1_800_000).unwrap();
    assert_eq!(
        store.list().unwrap()[0].state,
        tmux_sidecar::model::AgentState::Working
    );
}

#[test]
fn hook_captured_before_first_poll_keeps_run_timestamps_ordered() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&signal(1, "Stop", 200)).unwrap();
    let mut polled = store.list().unwrap().remove(0);
    polled.id.clear();
    polled.agent_pid = 2;
    polled.pane_id = "%2".into();
    polled.state_source = "process".into();
    let id = store.insert(&polled).unwrap();
    store.signal(&signal(2, "UserPromptSubmit", 100)).unwrap();
    let run = store.get(id.parse().unwrap(), 1).unwrap().unwrap();
    assert_eq!(run.state_since, 100);
    assert!(run.started_at <= run.state_since);
    store.signal(&signal(2, "PostToolUse", 50)).unwrap();
    assert_eq!(
        store
            .get(id.parse().unwrap(), 1)
            .unwrap()
            .unwrap()
            .started_at,
        100
    );
}
