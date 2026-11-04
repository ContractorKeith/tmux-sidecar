use super::store_worker::StoreWorker;
use crate::hook::{signal::Signal, spool};
use std::{io, path::PathBuf};
pub async fn replay(state: PathBuf, worker: &StoreWorker) -> io::Result<()> {
    let dir = state.clone();
    let bytes = tokio::task::spawn_blocking(move || spool::read(&dir))
        .await
        .map_err(io::Error::other)??;
    if bytes.is_empty() {
        return Ok(());
    }
    // Leave an incomplete final frame for a future append, never discard it.
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    if end == 0 {
        return Ok(());
    }
    let mut signals = Vec::new();
    for line in bytes[..end]
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
    {
        let value: serde_json::Value = serde_json::from_slice(line)?;
        let signal: Signal = serde_json::from_value(value["params"].clone())?;
        signal.validate()?;
        signals.push(signal);
    }
    for signal in signals {
        worker.with(move |store| store.signal(&signal)).await??;
    }
    let prefix = bytes[..end].to_vec();
    tokio::task::spawn_blocking(move || spool::acknowledge(&state, &prefix))
        .await
        .map_err(io::Error::other)?
}
