use super::*;
use crate::{hook::signal::Signal, model::AgentKind};
fn hook() -> Signal {
    Signal {
        server_start: 1,
        pane_id: "%0".into(),
        agent: AgentKind::Claude,
        agent_pid: 1,
        at: 1000,
        source: "hook:claude".into(),
        nonce: "00000000000000000000000000000001".into(),
        event: "PreToolUse".into(),
        detail: None,
        agent_session_id: None,
        cwd: "/work".into(),
        transcript_path: None,
        payload: serde_json::json!({}),
    }
}
#[tokio::test(flavor = "current_thread")]
async fn hooked_runs_do_not_invoke_scanner() {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("db")).await.unwrap();
    worker.with(|s| s.signal(&hook())).await.unwrap().unwrap();
    tick(&worker, |_, _| {
        panic!("hooked run must not scan transcripts")
    })
    .await
    .unwrap();
    worker.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn busy_scanner_drops_ticks_and_does_not_block_process_end() {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("db")).await.unwrap();
    worker
        .with(|s| {
            s.signal(&hook())?;
            Ok::<_, io::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
    // A process-only run: no hook evidence, using a distinct pane/pid.
    worker
        .with(|s| {
            let mut run = s.active()?.remove(0);
            run.id = String::new();
            run.pane_id = "%1".into();
            run.agent_pid = 2;
            s.insert(&run)
        })
        .await
        .unwrap()
        .unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let (started, ready) = tokio::sync::oneshot::channel();
    let mut reader = Reader::default();
    reader.start_with(worker.clone(), root.path().to_owned(), move |_, _| {
        let _ = started.send(());
        wait.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        Vec::new()
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), ready)
        .await
        .unwrap()
        .unwrap();
    reader.start_with(worker.clone(), root.path().to_owned(), |_, _| {
        panic!("busy scanner must drop another tick")
    });
    tokio::time::timeout(
        std::time::Duration::from_millis(300),
        worker.with(|s| s.end("r2", 2000)),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let state = worker
        .with(|s| s.get("r2".parse().unwrap(), 1))
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .state;
    assert_eq!(state, crate::model::AgentState::Done);
    release.send(()).unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        reader.running.take().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    worker.close().await.unwrap();
}
