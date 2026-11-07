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
    let result = if method == "wait" {
        exchange(&mut stream, method, params).await
    } else {
        let seconds = if method == "send" {
            rpc::SEND_TIMEOUT_SECS + 2
        } else {
            5
        };
        tokio::time::timeout(
            Duration::from_secs(seconds),
            exchange(&mut stream, method, params),
        )
        .await
        .unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("RPC {method} exceeded {seconds} s"),
            ))
        })
    };
    result.map_err(|e| connected_error(method, e))
}
pub(crate) fn connected_error(method: &str, error: io::Error) -> io::Error {
    if error
        .get_ref()
        .and_then(|e| e.downcast_ref::<rpc::RpcError>())
        .is_some_and(|e| e.code != rpc::DAEMON_UNREACHABLE)
    {
        return error;
    }
    let message = match method {
        "send" => "send outcome unknown — check the pane before retrying".to_owned(),
        "wait" => "daemon stopped while waiting".to_owned(),
        _ => format!("RPC {method} connection failed: {error}"),
    };
    io::Error::other(rpc::RpcError {
        code: rpc::ERROR,
        message,
        data: None,
    })
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
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(io::Error::new(error.kind(), "daemon not running"));
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
    warn_version(&hello);
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
    let bytes = rpc::request_frame(method, params)?;
    stream.get_mut().write_all(&bytes).await?;
    let mut line = String::new();
    if stream.read_line(&mut line).await? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "daemon connection closed",
        ));
    }
    let response: rpc::Response = serde_json::from_str(&line)?;
    if response.id != Some(rpc::Id::Number(1)) {
        return Err(io::Error::other("mismatched RPC response id"));
    }
    match response.outcome {
        rpc::Outcome::Success { result } => Ok(result),
        rpc::Outcome::Failure { error } => Err(io::Error::other(error)),
    }
}

// One warning per client process, including UI reconnects and MCP calls.
fn warn_version(hello: &Value) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if let Some(version) = hello["daemon_version"].as_str()
        && (version != env!("CARGO_PKG_VERSION")
            || hello["build_id"].as_str() != Some(env!("TMUX_SIDECAR_BUILD_ID")))
        && !WARNED.swap(true, Ordering::Relaxed)
    {
        eprintln!(
            "tmux-sidecar: daemon version {version} ({}) differs from client {} ({}); run `tmux-sidecar daemon stop` to restart",
            hello["build_id"].as_str().unwrap_or("unknown build"),
            env!("CARGO_PKG_VERSION"),
            env!("TMUX_SIDECAR_BUILD_ID")
        );
    }
}
