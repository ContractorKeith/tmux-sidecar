use super::*;
#[tokio::test(flavor = "current_thread")]
async fn run_locks_are_shared_and_do_not_block_sqlite_reads() {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("locks.db"))
        .await
        .unwrap();
    let guard = worker.lock_run("r1").await;
    let other = worker.clone();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            other.run_lock("r1").lock_owned()
        )
        .await
        .is_err()
    );
    let second = worker.lock_run("r2").await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), worker.with(|_| ()))
            .await
            .is_err()
    );
    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        worker.read(|s| s.active()),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    drop(guard);
    drop(second);
    let reacquired = other.run_lock("r1").lock_owned().await;
    drop(reacquired);
    drop(other);
    worker.close().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn sqlite_jobs_run_on_the_blocking_owner_and_close_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let runtime_thread = std::thread::current().id();
    let worker = StoreWorker::open(root.path().join("worker.db"))
        .await
        .unwrap();
    let owner = worker
        .with(|store| {
            store.active().unwrap();
            std::thread::current().id()
        })
        .await
        .unwrap();
    assert_ne!(owner, runtime_thread);
    let second = worker.with(|_| std::thread::current().id()).await.unwrap();
    assert_eq!(owner, second);
    worker.close().await.unwrap();
}

#[tokio::test]
async fn send_rollback_publishes_state_but_never_notifies() {
    let root = tempfile::tempdir().unwrap();
    let worker = StoreWorker::open(root.path().join("db")).await.unwrap();
    worker.with(|s|{
        let signal=serde_json::from_value(serde_json::json!({"server_start":1,"pane_id":"%0","agent":"claude","agent_pid":42,"at":100,"nonce":format!("{:032x}",100),"source":"hook:claude","event":"Stop","cwd":"/work","payload":{}})).unwrap();
        s.signal(&signal)?;let run=s.active()?.remove(0);s.ack(&run,150)?;
        let run=s.active()?.remove(0);s.sent(&run,200)
    }).await.unwrap().unwrap();
    worker.read(|_| ()).await.unwrap();
    let mut events = worker.events.subscribe();
    let mut notices = worker.notifications.subscribe();
    worker
        .with(|s| s.maintain(&[], 30_201, 1_800_000))
        .await
        .unwrap()
        .unwrap();
    worker.read(|_| ()).await.unwrap();
    let event = events.try_recv().unwrap();
    assert_eq!(event["to"], "waiting");
    assert_eq!(event["notify"], false);
    assert!(matches!(
        notices.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    worker.close().await.unwrap();
}
