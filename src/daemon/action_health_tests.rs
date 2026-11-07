use super::*;
async fn stale_action(method: &str) {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("db")).await.unwrap();
    worker.with(|s|s.signal(&serde_json::from_value(json!({"server_start":1,"pane_id":"%0","agent":"claude","agent_pid":1,"at":1,"source":"hook:claude","nonce":format!("{:032x}",1),"event":"Stop","cwd":"/","payload":{}})).unwrap())).await.unwrap().unwrap();
    worker.poll_error.send_replace(Some("probe-broken".into()));
    let params = if method == "wait" {
        json!({"sel":"r1","until":"waiting"})
    } else {
        json!({"sel":"r1","text":"unsafe","force":true})
    };
    let (reply, receive) = tokio::sync::oneshot::channel();
    let call = Call {
        request: rpc::Request {
            jsonrpc: rpc::Version::V2,
            id: rpc::Id::Number(1),
            method: method.into(),
            params: Some(params),
        },
        reply,
    };
    tokio::time::timeout(
        Duration::from_secs(2),
        serve(
            call,
            worker.clone(),
            root.path().join("nonexistent-tmux-socket"),
            1,
        ),
    )
    .await
    .unwrap();
    let response = receive.await.unwrap();
    assert!(
        serde_json::to_string(&response)
            .unwrap()
            .contains("daemon can't reach tmux: probe-broken")
    );
    worker.close().await.unwrap();
}
#[tokio::test]
async fn stale_wait_does_not_return_cached_waiting() {
    stale_action("wait").await;
}
#[tokio::test]
async fn stale_send_refuses_even_force_before_terminal_probe() {
    stale_action("send").await;
}
