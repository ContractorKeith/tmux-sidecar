//! One blocking owner keeps SQLite open, queries, transactions and close off tokio.
use crate::store::Store;
use std::{io, path::PathBuf, sync::mpsc};
use tokio::sync::{broadcast, oneshot};
type Job = Box<dyn FnOnce(&Store) + Send>;
pub struct StoreWorker {
    sender: mpsc::Sender<Job>,
    pub events: broadcast::Sender<serde_json::Value>,
    done: Option<oneshot::Receiver<()>>,
}
impl StoreWorker {
    pub async fn open(path: PathBuf) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let (ready, result) = oneshot::channel();
        let (closed, done) = oneshot::channel();
        let (events, _) = broadcast::channel(256);
        let publish = events.clone();
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
            while let Ok(job) = receiver.recv() {
                job(&store);
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
            done: Some(done),
            events,
        })
    }
    pub async fn with<T: Send + 'static>(
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
            events: self.events.clone(),
            done: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
