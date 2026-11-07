use super::{methods, store_worker::StoreWorker, transport::failure};
use crate::{
    hook::signal::Signal,
    rpc::{self, Outcome, Request, Response, Version},
};
use std::{io, path::PathBuf};
pub async fn record(worker: &StoreWorker, signal: Signal, state: PathBuf) -> io::Result<()> {
    let dropped = worker
        .with(move |store| store.record_signal(&signal))
        .await??;
    if dropped {
        tokio::task::spawn_blocking(move || {
            crate::hook::log(&state, &"orphan hook dropped: no unambiguous session, parent client, or unidentified cwd owner")
        }).await.map_err(io::Error::other)??;
    }
    Ok(())
}
pub async fn respond(request: Request, worker: &StoreWorker, state: PathBuf) -> Response {
    let id = Some(request.id.clone());
    match methods::prepare(&request).expect("signal delivery only") {
        Err(error) => failure(id, rpc::INVALID_PARAMS, error),
        Ok(signal) => match record(worker, signal, state).await {
            Ok(()) => Response {
                jsonrpc: Version::V2,
                id,
                outcome: Outcome::Success {
                    result: serde_json::json!({}),
                },
            },
            Err(error) => failure(id, rpc::INTERNAL_ERROR, error.to_string()),
        },
    }
}
