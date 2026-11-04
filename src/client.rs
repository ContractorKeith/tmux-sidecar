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
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut stream = BufReader::new(stream);
        let hello = exchange(&mut stream, "hello", serde_json::json!({})).await?;
        let key = paths::server_key(socket)?;
        if hello["server_key"].as_str() != Some(&key) {
            return Err(io::Error::other(format!(
                "wrong tmux server at TMUX_SIDECAR_RUNTIME_DIR {}; expected {key}, received {}",
                paths.runtime.display(),
                hello["server_key"]
            )));
        }
        if method == "hello" {
            return Ok(hello);
        }
        exchange(&mut stream, method, params).await
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "RPC {method} from {} exceeded 2 s",
                paths.runtime.join("sock").display()
            ),
        )
    })?
}
async fn connect(paths: &Paths) -> io::Result<UnixStream> {
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
async fn exchange(
    stream: &mut BufReader<UnixStream>,
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
