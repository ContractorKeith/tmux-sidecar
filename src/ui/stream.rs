//! Subscribe before loading: changes during the snapshot remain queued on the socket.
use super::{Group, Run, Snapshot};
use crate::{client, paths::Paths};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::UnixStream,
    sync::watch,
};
async fn line(reader: &mut BufReader<UnixStream>) -> io::Result<Value> {
    let mut bytes = Vec::new();
    let count = reader
        .take(1024 * 1024 + 1)
        .read_until(b'\n', &mut bytes)
        .await?;
    if count == 0 || count > 1024 * 1024 {
        return Err(io::Error::other("subscription closed or oversized"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
async fn snapshot(socket: &Path, paths: &Paths) -> io::Result<Snapshot> {
    let runs: Vec<Run> =
        serde_json::from_value(client::request(socket, paths, "list", json!({}), false).await?)?;
    let groups: Vec<Group> = serde_json::from_value(
        client::request(socket, paths, "group.ls", json!({}), false).await?,
    )?;
    Ok(Snapshot {
        runs,
        groups,
        connected: true,
        error: String::new(),
    })
}
async fn connected(socket: &Path, paths: &Paths, send: &watch::Sender<Snapshot>) -> io::Result<()> {
    let mut reader = client::open(socket, paths, true).await?;
    let reply = tokio::time::timeout(
        Duration::from_secs(2),
        client::exchange(&mut reader, "subscribe", json!({"views":true})),
    )
    .await
    .map_err(io::Error::other)??;
    if reply["subscribed"] != true {
        return Err(io::Error::other("subscribe refused"));
    }
    send.send_replace(snapshot(socket, paths).await?);
    loop {
        let event = line(&mut reader).await?;
        match event["method"].as_str() {
            Some("view_changed" | "state_changed") => {
                send.send_replace(snapshot(socket, paths).await?);
            }
            Some("subscription_error") => return Err(io::Error::other("subscription overflow")),
            _ => (),
        }
    }
}
struct Backoff(Duration);
impl Default for Backoff {
    fn default() -> Self {
        Self(Duration::from_millis(500))
    }
}
impl Backoff {
    fn next(&mut self) -> Duration {
        let delay = self.0;
        self.0 = (delay * 2).min(Duration::from_secs(5));
        delay
    }
    fn reset(&mut self) {
        *self = Self::default();
    }
}
pub async fn run(socket: PathBuf, paths: Paths, send: watch::Sender<Snapshot>) {
    let mut backoff = Backoff::default();
    loop {
        let error = connected(&socket, &paths, &send)
            .await
            .expect_err("subscription ends only on error");
        if send.borrow().connected {
            backoff.reset();
        }
        send.send_modify(|data| {
            data.connected = false;
            data.error = error.to_string();
        });
        tokio::time::sleep(backoff.next()).await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_delay_caps_and_resets_after_success() {
        let mut retry = Backoff::default();
        let delays: Vec<_> = (0..6).map(|_| retry.next().as_millis()).collect();
        assert_eq!(delays, [500, 1000, 2000, 4000, 5000, 5000]);
        retry.reset();
        assert_eq!(retry.next(), Duration::from_millis(500));
    }
}
