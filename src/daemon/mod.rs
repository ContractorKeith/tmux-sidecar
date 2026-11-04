//! One socket owner and one poller per tmux server.
mod methods;
mod peer;
mod transport;
use crate::{
    client::FileLock,
    engine,
    paths::{self, Paths},
    proc::PsProcessTree,
    seams::{ProcessTree, Tmux},
    store::Store,
    tmux::{RealTmux, Socket},
};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::UnixListener, sync::mpsc, task::JoinSet};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn other(e: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(e)
}

struct RuntimeGuard(PathBuf);
impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("sock"));
        let _ = fs::remove_file(self.0.join("daemon.pid"));
    }
}

pub async fn run(socket: PathBuf, paths: Paths) -> io::Result<()> {
    paths::prepare(&paths)?;
    // Also protects foreground starts; never unlink a live daemon's socket.
    let _lock = FileLock::try_acquire(&paths.runtime.join("daemon.lock"))?;
    let _guard = RuntimeGuard(paths.runtime.clone());
    match fs::remove_file(paths.runtime.join("sock")) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let store = Store::open(&paths.state.join("tmux-sidecar.db")).map_err(other)?;
    let poll_ms = poll_ms(&paths)?;
    if !poll(&socket, &store).await? {
        store.end_all(now()).map_err(other)?;
        return Err(io::Error::other("tmux server is not running"));
    }
    let listener = UnixListener::bind(paths.runtime.join("sock"))?;
    fs::set_permissions(
        paths.runtime.join("sock"),
        fs::Permissions::from_mode(0o600),
    )?;
    fs::write(
        paths.runtime.join("daemon.pid"),
        std::process::id().to_string(),
    )?;
    let key = paths::server_key(&socket)?;
    let (sender, mut receiver) = mpsc::channel::<transport::Call>(64);
    let mut connections = JoinSet::new();
    let mut interval = tokio::time::interval(Duration::from_millis(poll_ms));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = loop {
        tokio::select! {
            _ = term.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
            _ = interval.tick() => match poll(&socket, &store).await {
                Ok(true) => (), Ok(false) => break Ok(()), Err(e) => break Err(e),
            },
            incoming = listener.accept() => match incoming {
                Ok((stream, _)) => { let sender = sender.clone(); connections.spawn(async move { transport::serve(stream, sender).await }); },
                Err(e) => break Err(e),
            },
            Some(call) = receiver.recv() => {
                let stop = call.request.method == "daemon.stop" && methods::empty_params(&call.request);
                let response = methods::handle(call.request, &store, &key, poll_ms);
                let _ = call.reply.send(response);
                if stop { break Ok(()); }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => (),
        }
    };
    store.end_all(now()).map_err(other)?;
    drop(listener);
    // Let the stop response reach its client, then close any idle connections.
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.shutdown().await;
    result
}
async fn poll(socket: &std::path::Path, store: &Store) -> io::Result<bool> {
    let socket = socket.to_path_buf();
    let snapshot = tokio::task::spawn_blocking(move || {
        let tmux = RealTmux {
            socket: Socket::Path(socket),
        };
        let panes = match tmux.list_panes() {
            Ok(panes) => panes,
            Err(e) => {
                // A failed exec is an operational error; a failed tmux invocation means its server is gone.
                if e.kind() == io::ErrorKind::Other {
                    return Ok(None);
                }
                return Err(e);
            }
        };
        Ok(Some((panes, PsProcessTree.snapshot()?)))
    })
    .await
    .map_err(other)??;
    let Some((panes, processes)) = snapshot else {
        return Ok(false);
    };
    let now = now();
    let changes = engine::detect(&panes, &processes, &store.active().map_err(other)?, now);
    for pane in panes {
        store
            .pane(pane.server_start, &pane.pane_id)
            .map_err(other)?;
    }
    for id in changes.end {
        store.end(&id, now).map_err(other)?;
    }
    for run in changes.start {
        store.insert(&run).map_err(other)?;
    }
    Ok(true)
}
fn poll_ms(paths: &Paths) -> io::Result<u64> {
    let config = match fs::read_to_string(&paths.config) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };
    let value = if let Ok(value) = std::env::var("TMUX_SIDECAR_POLL_MS") {
        value.parse::<u64>().map_err(other)?
    } else if let Some(value) = config.get("poll_ms") {
        value
            .as_integer()
            .and_then(|n| n.try_into().ok())
            .ok_or_else(|| io::Error::other("poll_ms must be a positive integer"))?
    } else {
        1000
    };
    if value == 0 {
        return Err(io::Error::other("poll_ms must be positive"));
    }
    Ok(value)
}
