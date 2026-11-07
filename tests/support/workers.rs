use std::{any::Any, sync::mpsc::Receiver, thread::JoinHandle, time::Instant};
type Failure = Box<dyn Any + Send>;
pub fn finish(
    handles: Vec<JoinHandle<()>>,
    results: Receiver<Result<(), Failure>>,
    deadline: Instant,
) {
    let mut failed = None;
    for _ in 0..handles.len() {
        match results.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(())) => (),
            Ok(Err(error)) => {
                if failed.is_none() {
                    failed = Some(error);
                }
            }
            Err(error) => {
                if failed.is_none() {
                    failed = Some(Box::new(format!("worker deadline: {error}")) as Failure);
                }
                break;
            }
        }
    }
    for handle in handles {
        if let Err(error) = handle.join()
            && failed.is_none()
        {
            failed = Some(error);
        }
    }
    if let Some(error) = failed {
        std::panic::resume_unwind(error);
    }
}
#[test]
fn joins_cleanup_before_propagating_expired_deadline() {
    let cleaned = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let marker = cleaned.clone();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        marker.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = send.send(Ok(()));
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        finish(vec![worker], receive, Instant::now())
    }));
    assert!(result.is_err());
    assert!(cleaned.load(std::sync::atomic::Ordering::SeqCst));
}
