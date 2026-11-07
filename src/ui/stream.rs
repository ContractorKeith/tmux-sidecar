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
pub async fn run(socket: PathBuf, paths: Paths, send: watch::Sender<Snapshot>) {
    loop {
        let _ = connected(&socket, &paths, &send).await;
        send.send_modify(|data| data.connected = false);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
