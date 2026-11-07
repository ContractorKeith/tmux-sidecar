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
    pub notifications: broadcast::Sender<serde_json::Value>,
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
        let (notifications, _) = broadcast::channel(256);
        let notify = notifications.clone();
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
            let mut view = store.view_digest().ok();
            while let Ok(job) = receiver.recv() {
                let before = store.total_changes();
                job(&store);
                if store.total_changes() != before {
                    let next = store.view_digest().ok();
                    if next != view {
                        view = next;
                        invalidate.send_modify(|revision| *revision = revision.wrapping_add(1));
                    }
                }
                if let Ok(changes) = store.changes_since(cursor) {
                    for (id, payload) in changes {
                        cursor = id;
                        if payload["notify"] != false {
                            let _ = notify.send(payload.clone());
                        }
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
            notifications,
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
            notifications: self.notifications.clone(),
            views: self.views.clone(),
            done: None,
        }
    }
}

#[cfg(test)]
#[path = "store_worker_tests.rs"]
mod tests;
