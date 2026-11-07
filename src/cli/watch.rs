use super::execute::{Failure, response_error};
use crate::{client, paths::Paths};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
pub async fn run(socket: &Path, paths: &Paths, json: bool, quiet: bool) -> Result<(), Failure> {
    let mut stream = client::open(socket, paths, true)
        .await
        .map_err(response_error)?;
    client::exchange(&mut stream, "subscribe", serde_json::json!({}))
        .await
        .map_err(|e| response_error(client::connected_error("watch", e)))?;
    loop {
        let mut bytes = Vec::new();
        let mut limited = (&mut stream).take(1024 * 1024 + 1);
        let count = tokio::select! {
            signal = tokio::signal::ctrl_c() => { signal.map_err(|e| (1,e.to_string()))?; return Ok(()); }
            count = limited.read_until(b'\n',&mut bytes) => count.map_err(|e| (1,e.to_string()))?,
        };
        if count == 0 {
            return Err((1, "daemon exited".into()));
        }
        if count - usize::from(bytes.last() == Some(&b'\n')) > 1024 * 1024 {
            return Err((1, "subscription frame exceeds 1 MiB".into()));
        }
        let frame: crate::rpc::Notification =
            serde_json::from_slice(&bytes).map_err(|e| (1, e.to_string()))?;
        if frame.method != "state_changed" {
            return Err((1, format!("subscription ended: {:?}", frame.params)));
        }
        if !quiet {
            super::output::print("state_changed", &frame.params.unwrap_or_default(), json)
                .map_err(super::execute::stdout_error)?;
        }
    }
}
