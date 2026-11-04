//! Finish signals already accepted before closing SQLite; idle clients have a deadline.
use super::{
    methods,
    store_worker::StoreWorker,
    transport::{self, Call},
};
use std::{io, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};
pub async fn drain(
    connections: &mut JoinSet<io::Result<()>>,
    receiver: &mut mpsc::Receiver<Call>,
    worker: &StoreWorker,
    key: &str,
    poll_ms: u64,
) -> io::Result<()> {
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        while !connections.is_empty() {
            tokio::select! {
                Some(call)=receiver.recv()=> {
                    let response=if call.request.method=="signal" {
                        let prepared=methods::prepare(&call.request);
                        let key=key.to_owned();
                        worker.with(move |store| {
                            methods::handle_prepared(call.request, store, &key, poll_ms, prepared)
                        }).await?
                    } else {
                        transport::failure(
                            Some(call.request.id),
                            crate::rpc::DAEMON_UNREACHABLE,
                            "daemon is stopping",
                        )
                    };
                    let _=call.reply.send(response);
                },
                Some(_)=connections.join_next()=>(),
            }
        }
        Ok(())
    })
    .await;
    connections.shutdown().await;
    result.unwrap_or(Ok(()))
}
