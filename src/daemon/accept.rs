//! Bound connections and keep descriptor exhaustion from ending the daemon.
use super::{
    store_worker::StoreWorker,
    transport::{self, Call},
};
use std::{fs::File, io, path::Path, sync::Arc, time::Duration};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time::Instant,
};
pub(super) struct Acceptor {
    slots: Arc<Semaphore>,
    after: Instant,
    delay: u64,
    spare: Option<File>,
}
impl Acceptor {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            slots: Arc::new(Semaphore::new(256)),
            after: Instant::now(),
            delay: 50,
            spare: Some(File::open("/dev/null")?),
        })
    }
    pub async fn accept(&self, listener: &UnixListener) -> io::Result<UnixStream> {
        tokio::time::sleep_until(self.after).await;
        listener.accept().await.map(|(stream, _)| stream)
    }
    pub fn start(
        &mut self,
        stream: UnixStream,
        sender: mpsc::Sender<Call>,
        store: &StoreWorker,
        tasks: &mut JoinSet<io::Result<()>>,
    ) {
        self.delay = 50;
        match self.slots.clone().try_acquire_owned() {
            Ok(permit) => {
                let events = store.events.subscribe();
                let views = store.views.subscribe();
                tasks.spawn(async move {
                    let _permit = permit;
                    transport::serve(stream, sender, events, views).await
                });
            }
            Err(_) => {
                tasks.spawn(reject(stream));
            }
        }
    }
    pub fn retry(&mut self, error: io::Error, state: &Path) {
        // Free one descriptor so EMFILE itself can be recorded in the rotating log.
        drop(self.spare.take());
        let _ = super::log::write(state, &format!("accept failed; retrying: {error}"));
        self.spare = File::open("/dev/null").ok();
        self.after = Instant::now() + Duration::from_millis(self.delay);
        self.delay = (self.delay * 2).min(500);
    }
}

async fn reject(mut stream: UnixStream) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let frame=b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"daemon connection limit reached (256); close idle clients\"}}\n";
    let _ = tokio::time::timeout(Duration::from_millis(50), stream.write_all(frame)).await;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    #[tokio::test]
    async fn rejected_peer_gets_frame_after_write_backpressure() {
        tokio::time::timeout(Duration::from_secs(2), async {
            let (server, mut client) = UnixStream::pair().unwrap();
            server.writable().await.unwrap();
            let padding = [b'x'; 4096];
            let mut count = 0;
            while let Ok(n) = server.try_write(&padding) {
                count += n;
            }
            assert!(count > 0);
            // The old immediate nonblocking write loses the frame before the reader drains.
            let rejecting = tokio::spawn(reject(server));
            tokio::task::yield_now().await;
            let mut bytes = Vec::new();
            client.read_to_end(&mut bytes).await.unwrap();
            rejecting.await.unwrap().unwrap();
            let frame: serde_json::Value = serde_json::from_slice(&bytes[count..]).unwrap();
            assert_eq!(frame["error"]["code"], -32600);
        })
        .await
        .unwrap();
    }
}
