use super::*;
use crate::hook::signal::Signal;
async fn worker() -> (tempfile::TempDir, StoreWorker) {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("actions.db"))
        .await
        .unwrap();
    worker
        .with(|s| s.signal(&signal("UserPromptSubmit", 1)))
        .await
        .unwrap()
        .unwrap();
    (root, worker)
}
fn signal(event: &str, at: u64) -> Signal {
    serde_json::from_value(json!({"server_start":1,"pane_id":"%0","agent":"claude","agent_pid":1,
        "at":at,"source":"hook:claude","nonce":format!("{at:032x}"),"event":event,"cwd":"/","payload":{}})).unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn canceled_wait_releases_its_subscription() {
    let (_root, worker) = worker().await;
    let (reply, receive) = tokio::sync::oneshot::channel();
    let call = Call {
        request: rpc::Request {
            jsonrpc: rpc::Version::V2,
            id: rpc::Id::Number(1),
            method: "wait".into(),
            params: Some(json!({"sel":"r1","until":"waiting"})),
        },
        reply,
    };
    let task = tokio::spawn(serve(call, worker.clone(), PathBuf::new(), 1));
    tokio::time::timeout(Duration::from_secs(1), async {
        while worker.events.receiver_count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(receive);
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(worker.events.receiver_count(), 0);
    worker.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn wait_observes_a_transient_target_before_done() {
    let (_root, worker) = worker().await;
    {
        let pending = wait(
            &worker,
            Wait {
                sel: "r1".into(),
                until: AgentState::Waiting,
                timeout: Some(1),
            },
            1,
        );
        tokio::pin!(pending);
        // Prime the snapshot and subscription without advancing the state yet.
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pending)
                .await
                .is_err()
        );
        worker
            .with(|s| {
                s.signal(&signal("Stop", 2)).unwrap();
                s.signal(&signal("SessionEnd", 3)).unwrap();
            })
            .await
            .unwrap();
        assert_eq!(
            pending.await.unwrap(),
            json!({"run_id":"r1","state":"waiting","at":2})
        );
    }
    worker.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn wait_deadline_includes_selector_read() {
    let (_root, worker) = worker().await;
    let (release, block) = std::sync::mpsc::channel();
    let (began, started) = tokio::sync::oneshot::channel();
    let owner = worker.clone();
    let blocked = tokio::spawn(async move {
        owner
            .read(move |_| {
                let _ = began.send(());
                let _ = block.recv_timeout(Duration::from_secs(1));
            })
            .await
            .unwrap()
    });
    started.await.unwrap();
    let began = std::time::Instant::now();
    let result = wait(
        &worker,
        Wait {
            sel: "r1".into(),
            until: AgentState::Blocked,
            timeout: Some(0),
        },
        1,
    )
    .await;
    assert_eq!(result.unwrap_err().0, rpc::TIMEOUT);
    assert!(began.elapsed() < Duration::from_millis(200));
    release.send(()).unwrap();
    blocked.await.unwrap();
    worker.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn ended_wait_reports_the_stored_state() {
    let (_root, worker) = worker().await;
    let id = worker
        .with(|s| {
            let mut ended = s.active().unwrap().remove(0);
            ended.agent_pid = 2;
            ended.ended_at = Some(2);
            ended.state = AgentState::Error;
            s.insert(&ended).unwrap()
        })
        .await
        .unwrap();
    let error = wait(
        &worker,
        Wait {
            sel: id,
            until: AgentState::Waiting,
            timeout: Some(1),
        },
        1,
    )
    .await
    .unwrap_err();
    assert_eq!(error.0, rpc::ERROR);
    assert!(error.1.contains("ended in error"));
    worker.close().await.unwrap();
}
