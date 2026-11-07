//! One blocking owner keeps SQLite open, queries, transactions and close off tokio.
use crate::store::Store;
use std::{io, path::PathBuf, sync::mpsc};
use tokio::sync::{broadcast, oneshot};
type Job = Box<dyn FnOnce(&Store) + Send>;
pub struct StoreWorker {
    sender: mpsc::Sender<Job>,
    mutation: std::sync::Arc<tokio::sync::Mutex<()>>,
    locks: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Weak<tokio::sync::Mutex<()>>>,
        >,
    >,
    pub events: broadcast::Sender<serde_json::Value>,
    pub views: tokio::sync::watch::Sender<u64>,
    done: Option<oneshot::Receiver<()>>,
}
impl StoreWorker {
    pub async fn open(path: PathBuf) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let (ready, result) = oneshot::channel();
        let (closed, done) = oneshot::channel();
        let (events, _) = broadcast::channel(256);
        let publish = events.clone();
        let (views, _) = tokio::sync::watch::channel(0u64);
        let invalidate = views.clone();
        tokio::task::spawn_blocking(move || {
            let store = match Store::open(&path) {
                Ok(store) => store,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return;
                }
            };
            if ready.send(Ok(())).is_err() {
                return;
            }
            let mut cursor = store
                .changes_since(0)
                .ok()
                .and_then(|rows| rows.last().map(|r| r.0))
                .unwrap_or(0);
            let mut view = store.view().ok();
            while let Ok(job) = receiver.recv() {
                let before = store.total_changes();
                job(&store);
                if store.total_changes() != before {
                    let next = store.view().ok();
                    if next != view {
                        view = next;
                        invalidate.send_modify(|revision| *revision = revision.wrapping_add(1));
                    }
                }
                if let Ok(changes) = store.changes_since(cursor) {
                    for (id, payload) in changes {
                        cursor = id;
                        let _ = publish.send(payload);
                    }
                }
            }
            drop(store);
            let _ = closed.send(());
        });
        result.await.map_err(io::Error::other)??;
        Ok(Self {
            sender,
            mutation: Default::default(),
            locks: Default::default(),
            done: Some(done),
            events,
            views,
        })
    }
    pub async fn with<T: Send + 'static>(
        &self,
        job: impl FnOnce(&Store) -> T + Send + 'static,
    ) -> io::Result<T> {
        let _guard = self.mutation.lock().await;
        // Only sends hold run locks. Acquire those locks in stable order; the
        // mutation gate prevents a send registering a new lock until this job ends.
        let mut locks: Vec<_> = self
            .locks
            .lock()
            .expect("run lock registry poisoned")
            .iter()
            .filter_map(|(id, lock)| lock.upgrade().map(|lock| (id.clone(), lock)))
            .collect();
        locks.sort_by(|a, b| a.0.cmp(&b.0));
        let mut guards = Vec::new();
        for (_, lock) in locks {
            guards.push(lock.lock_owned().await);
        }
        self.read(job).await
    }
    pub async fn lock_run(&self, id: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let _gate = self.mutation.lock().await;
        self.run_lock(id).lock_owned().await
    }
    fn run_lock(&self, id: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().expect("run lock registry poisoned");
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(id).and_then(std::sync::Weak::upgrade) {
            return lock;
        }
        let lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(id.to_owned(), std::sync::Arc::downgrade(&lock));
        lock
    }
    pub async fn read<T: Send + 'static>(
        &self,
        job: impl FnOnce(&Store) -> T + Send + 'static,
    ) -> io::Result<T> {
        let (send, receive) = oneshot::channel();
        self.sender
            .send(Box::new(move |store| {
                let _ = send.send(job(store));
            }))
            .map_err(|_| io::Error::other("store worker stopped"))?;
        receive.await.map_err(io::Error::other)
    }
    pub async fn close(self) -> io::Result<()> {
        drop(self.sender);
        self.done
            .expect("only owner closes store worker")
            .await
            .map_err(io::Error::other)
    }
}

impl Clone for StoreWorker {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            mutation: self.mutation.clone(),
            locks: self.locks.clone(),
            events: self.events.clone(),
            views: self.views.clone(),
            done: None,
        }
    }
}

#[cfg(test)]
mod tests {
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
}
