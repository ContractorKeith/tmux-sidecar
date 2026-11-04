use super::store_worker::StoreWorker;
use crate::hook::{context, signal::Signal, spool};
use std::{io, path::PathBuf};
/// Replay failures are logged, never charged to tmux/process poll health.
pub async fn replay(state: PathBuf, runtime: PathBuf, socket: PathBuf, worker: &StoreWorker) {
    if let Err(error) = inner(state.clone(), runtime, socket, worker).await {
        let _ = tokio::task::spawn_blocking(move || {
            super::log::write(&state, &format!("spool replay: {error}"))
        })
        .await;
    }
}
async fn inner(
    state: PathBuf,
    runtime: PathBuf,
    socket: PathBuf,
    worker: &StoreWorker,
) -> io::Result<()> {
    let dir = state.clone();
    let claims = tokio::task::spawn_blocking(move || spool::claim(&dir))
        .await
        .map_err(io::Error::other)??;
    for claim in claims {
        replay_claim(
            state.clone(),
            runtime.clone(),
            socket.clone(),
            worker,
            claim,
        )
        .await?;
    }
    Ok(())
}
async fn replay_claim(
    state: PathBuf,
    runtime: PathBuf,
    socket: PathBuf,
    worker: &StoreWorker,
    claim: PathBuf,
) -> io::Result<()> {
    let dir = state.clone();
    let claimed = claim.clone();
    let (signals, rejected, pending) = tokio::task::spawn_blocking(move || -> io::Result<_> {
        let bytes = spool::read_claim(&claimed)?;
        let mut signals = Vec::new();
        let mut rejected = Vec::new();
        let mut pending = false;
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let parsed = (|| -> io::Result<Signal> {
                let value: serde_json::Value = serde_json::from_slice(line)?;
                if value["jsonrpc"] != "2.0" || value["method"] != "signal" {
                    return Err(io::Error::other("not a signal frame"));
                }
                let signal: Signal = serde_json::from_value(value["params"].clone())?;
                signal.validate_frame()?;
                Ok(signal)
            })();
            match parsed {
                Ok(mut signal) => {
                    if signal.server_start == 0 {
                        match context::read(&runtime, &socket) {
                            Ok(start) => signal.server_start = start,
                            Err(_) => {
                                pending = true;
                                continue;
                            }
                        }
                    }
                    signals.push(signal);
                }
                Err(error) => {
                    rejected.extend_from_slice(line);
                    if !line.ends_with(b"\n") {
                        rejected.push(b'\n');
                    }
                    let _ = super::log::write(&dir, &format!("rejected spool line: {error}"));
                }
            }
        }
        Ok((signals, rejected, pending))
    })
    .await
    .map_err(io::Error::other)??;

    let mut signals = signals;
    signals.sort_by_key(|signal| signal.at);
    for signal in signals {
        worker.with(move |store| store.signal(&signal)).await??;
    }
    tokio::task::spawn_blocking(move || {
        if !rejected.is_empty() {
            spool::reject(&state, &rejected)?;
        }
        if pending {
            return Ok(());
        }
        spool::acknowledge(&claim)
    })
    .await
    .map_err(io::Error::other)?
}
