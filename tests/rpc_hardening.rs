use serde_json::{Value, json};
use tmux_sidecar::{daemon, hook::signal::Signal, rpc, store::Store};
fn request(store: &Store, method: &str, params: Value) -> rpc::Outcome {
    daemon::dispatch(
        rpc::Request {
            jsonrpc: rpc::Version::V2,
            id: rpc::Id::Number(1),
            method: method.into(),
            params: Some(params),
        },
        store,
        "test",
        100,
    )
    .outcome
}
fn result(store: &Store, method: &str, params: Value) -> Value {
    if method == "attention" {
        return serde_json::to_value(store.attention().unwrap()).unwrap();
    }
    match request(store, method, params) {
        rpc::Outcome::Success { result } => result,
        other => panic!("{other:?}"),
    }
}
fn seed(store: &Store, pid: u32) {
    store
        .signal(&Signal {
            server_key: None,
            pane_verified: true,
            server_start: 1,
            pane_id: format!("%{pid}"),
            agent: tmux_sidecar::model::AgentKind::Claude,
            agent_pid: pid,
            at: 1000,
            nonce: format!("{pid:032x}"),
            source: "hook:claude".into(),
            event: "PermissionRequest".into(),
            detail: None,
            agent_session_id: None,
            cwd: "/work".into(),
            transcript_path: None,
            orphaned: false,
            parent_agent_pid: None,
            payload: json!({}),
        })
        .unwrap();
}
#[test]
fn attention_ties_follow_numeric_rowid() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    for pid in 1..=12 {
        seed(&store, pid);
    }
    let queue = result(&store, "attention", json!({}));
    assert_eq!(queue[8]["id"], "r9");
    assert_eq!(queue[9]["id"], "r10");
}
#[test]
fn removing_absent_group_does_not_create_it() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    seed(&store, 1);
    result(&store, "group.rm", json!({"group":"absent","sel":"r1"}));
    assert_eq!(result(&store, "group.ls", json!({})), json!([]));
}
#[test]
fn validate_pane_and_group_names_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    seed(&store, 1);
    for name in ["".to_owned(), "a\nb".into(), "x".repeat(65)] {
        for (method, params) in [
            ("name", json!({"sel":"r1","name":name})),
            ("group.add", json!({"sel":"r1","group":name})),
            ("group.rm", json!({"sel":"r1","group":name})),
        ] {
            assert!(
                matches!(request(&store,method,params),rpc::Outcome::Failure{error} if error.code==rpc::INVALID_PARAMS),
                "{method} {name:?}"
            );
        }
    }
    result(&store, "name", json!({"sel":"r1","name":"界".repeat(64)}));
    result(&store, "name", json!({"sel":"r1","name":null}));
    assert_eq!(store.pane_name(1, "%1").unwrap(), None);
    assert_eq!(result(&store, "group.ls", json!({})), json!([]));
}
#[test]
fn already_read_focus_does_not_rewrite_mark() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    seed(&store, 1);
    let run = store.active().unwrap().remove(0);
    store.ack(&run, 2000).unwrap();
    let pane = tmux_sidecar::seams::PaneSnapshot {
        server_start: 1,
        pane_id: "%1".into(),
        pane_pid: 1,
        cwd: "/work".into(),
        current_command: "claude".into(),
        pane_active: true,
        window_active: true,
        session_attached: true,
    };
    store.maintain(&[pane], 3000, 1_800_000).unwrap();
    assert_eq!(store.active().unwrap()[0].read_at, Some(2000));
}
