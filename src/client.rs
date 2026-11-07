//! Every non-daemon reader uses this transport; only list auto-spawns in M1.
mod spawn;
use crate::{
    paths::{self, Paths},
    rpc,
};
use serde_json::Value;
pub use spawn::FileLock;
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

pub fn select_socket(explicit: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(socket) = explicit {
        return paths::canonical_socket(socket);
    }
    if let Some(socket) = std::env::var_os("TMUX").and_then(|s| paths::socket_from_tmux(&s)) {
        return paths::canonical_socket(&socket);
    }
    paths::canonical_socket(&crate::tmux::default_socket()?)
}

pub async fn request(
    socket: &Path,
    paths: &Paths,
    method: &str,
    params: Value,
    spawn: bool,
) -> io::Result<Value> {
    let mut stream = open(socket, paths, spawn).await?;
    if method == "wait" {
        return exchange(&mut stream, method, params).await;
    }
    tokio::time::timeout(
        Duration::from_secs(5),
        exchange(&mut stream, method, params),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("RPC {method} exceeded 5 s"),
        )
    })?
}
pub async fn open(socket: &Path, paths: &Paths, spawn: bool) -> io::Result<BufReader<UnixStream>> {
    let stream = match connect(paths).await {
        Ok(stream) => stream,
        Err(_) if spawn => {
            tokio::time::timeout(Duration::from_secs(2), spawn::ensure(socket, paths))
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "waiting for daemon startup (spawn.lock/socket) in {} exceeded 2 s",
                            paths.runtime.display()
                        ),
                    )
                })??;
            connect(paths).await?
        }
        Err(error) => return Err(error),
    };
    let mut stream = BufReader::new(stream);
    let hello = tokio::time::timeout(
        Duration::from_secs(2),
        exchange(&mut stream, "hello", serde_json::json!({})),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "hello exceeded 2 s"))??;
    let key = paths::server_key(socket)?;
    if hello["server_key"].as_str() != Some(&key) {
        return Err(io::Error::other(format!(
            "wrong tmux server at TMUX_SIDECAR_RUNTIME_DIR {}; expected {key}, received {}",
            paths.runtime.display(),
            hello["server_key"]
        )));
    }
    Ok(stream)
}

pub(crate) async fn connect(paths: &Paths) -> io::Result<UnixStream> {
    tokio::time::timeout(
        Duration::from_millis(250),
        UnixStream::connect(paths.runtime.join("sock")),
    )
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "connect to {} exceeded 250 ms",
                paths.runtime.join("sock").display()
            ),
        )
    })?
}
pub(crate) async fn verify(stream: &mut UnixStream, socket: &Path) -> io::Result<()> {
    let hello = exchange(&mut BufReader::new(stream), "hello", serde_json::json!({})).await?;
    if hello["server_key"].as_str() != Some(&paths::server_key(socket)?) {
        return Err(io::Error::other("wrong tmux server"));
    }
    Ok(())
}
pub use spawn::trigger_spawn;
pub(crate) async fn exchange<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: &mut BufReader<S>,
    method: &str,
    params: Value,
) -> io::Result<Value> {
    let request = rpc::Request {
        jsonrpc: rpc::Version::V2,
        id: rpc::Id::Number(1),
        method: method.into(),
        params: Some(params),
    };
    let mut bytes = serde_json::to_vec(&request)?;
    bytes.push(b'\n');
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other(rpc::RpcError {
            code: rpc::INVALID_PARAMS,
            message: "request exceeds 1 MiB".into(),
            data: None,
        }));
    }
    stream.get_mut().write_all(&bytes).await?;
    let mut line = String::new();
    stream.read_line(&mut line).await?;
    let response: rpc::Response = serde_json::from_str(&line)?;
    if response.id != Some(rpc::Id::Number(1)) {
        return Err(io::Error::other("mismatched RPC response id"));
    }
    match response.outcome {
        rpc::Outcome::Success { result } => Ok(result),
        rpc::Outcome::Failure { error } => Err(io::Error::other(error)),
    }
}
