use super::store_worker::StoreWorker;
pub(super) fn error(worker: &StoreWorker) -> Option<(i32, String)> {
    worker.poll_error.borrow().as_ref().map(|reason| {
        (
            crate::rpc::ERROR,
            format!("daemon can't reach tmux: {reason}"),
        )
    })
}
pub(super) async fn failed(worker: &StoreWorker) -> (i32, String) {
    let mut health = worker.poll_error.subscribe();
    loop {
        if let Some(error) = error(worker) {
            return error;
        }
        if health.changed().await.is_err() {
            return (crate::rpc::ERROR, "daemon stopped".into());
        }
    }
}
