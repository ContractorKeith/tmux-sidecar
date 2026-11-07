use serde_json::json;
use tmux_sidecar::{hook::signal::Signal, model::AgentState, store::Store};
fn hook(event: &str, at: u64, nonce: u64) -> Signal {
    serde_json::from_value(json!({"server_start":1,"pane_id":"%0","agent":"claude","agent_pid":42,"at":at,"nonce":format!("{nonce:032x}"),"source":"hook:claude","event":event,"cwd":"/work","payload":{}})).unwrap()
}
#[test]
fn a_new_hook_receipt_confirms_send_even_with_an_older_capture_timestamp() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    store.signal(&hook("Stop", 100, 1)).unwrap();
    let run = store.list().unwrap().remove(0);
    store.sent(&run, 200).unwrap();
    store.signal(&hook("UserPromptSubmit", 150, 2)).unwrap();
    store.maintain(&[], 30_201, 1_800_000).unwrap();
    assert_eq!(store.list().unwrap()[0].state, AgentState::Working);
}
#[test]
fn unconfirmed_send_restores_read_waiting_without_attention_or_notification() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(&root.path().join("db")).unwrap();
    let stop = hook("Stop", 100, 1);
    store.signal(&stop).unwrap();
    let run = store.list().unwrap().remove(0);
    store.ack(&run, 150).unwrap();
    let run = store.list().unwrap().remove(0);
    store.sent(&run, 200).unwrap();
    // A duplicate receipt is not new confirmation, even when replayed after send.
    store.signal(&stop).unwrap();
    store.maintain(&[], 30_201, 1_800_000).unwrap();
    let restored = store.list().unwrap().remove(0);
    assert_eq!(restored.state, AgentState::Waiting);
    assert_eq!(restored.read_at, Some(150));
    assert_eq!(restored.state_since, 100);
    let changes = store.changes_since(0).unwrap();
    assert_eq!(changes.last().unwrap().1["notify"], false);
}
