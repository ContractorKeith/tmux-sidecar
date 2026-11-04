//! One socket owner and one poller per tmux server.
pub mod log;
mod methods;
pub use methods::handle as dispatch;
mod peer;
mod poll;
mod replay;
mod store_worker;
mod transport;
use crate::{
    client::FileLock,
    paths::{self, Paths},
};
use std::{
    fs,
    future::Future,
    io,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::UnixListener, sync::mpsc, task::JoinSet};

type PollFuture<'a> = Pin<Box<dyn Future<Output = Result<bool, poll::Failure>> + 'a>>;

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
    let result = run_inner(socket, &paths).await;
    if let Err(error) = &result
        && paths::prepare(&paths).is_ok()
    {
        let _ = log::write(&paths.state, &error.to_string());
    }
    result
}
async fn run_inner(socket: PathBuf, paths: &Paths) -> io::Result<()> {
    paths::prepare(paths)?;
    // Also protects foreground starts; never unlink a live daemon's socket.
    let _lock = FileLock::try_acquire(&paths.runtime.join("daemon.lock"))?;
    let _guard = RuntimeGuard(paths.runtime.clone());
    match fs::remove_file(paths.runtime.join("sock")) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let db = paths.state.join("tmux-sidecar.db");
    if !db.exists() {
        log::write(
            &paths.state,
            &format!("creating fresh database {}", db.display()),
        )?;
    }
    let store = store_worker::StoreWorker::open(db).await?;
    replay::replay(paths.state.clone(), &store).await?;
    let poll_ms = poll_ms(paths)?;
    let mut health = poll::Health::default();
    let initial = poll::tick(socket.clone(), &store).await;
    health.record(&initial);
    match initial {
        Ok(false) => return Err(io::Error::other("tmux server is not running")),
        Err(f) if f.fatal => return Err(f.error),
        Err(f) => {
            log::write(&paths.state, &format!("initial poll failure: {}", f.error))?;
        }
        Ok(true) => (),
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
    let mut polling: Option<PollFuture<'_>> = None;
    let result = loop {
        tokio::select! {
            _ = term.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
            _ = interval.tick(), if polling.is_none() => {
                let socket = socket.clone();
                let state = paths.state.clone();
                let worker = &store;
                polling = Some(Box::pin(async move {
                    replay::replay(state, worker).await?;
                    poll::tick(socket, worker).await
                }));
            },
            result = async {
                match &mut polling {
                    Some(poll) => poll.await,
                    None => std::future::pending().await,
                }
            } => {
                polling = None;
                let stop = health.record(&result);
                match result {
                    Ok(true) => (), Ok(false) => break Ok(()), Err(failure) => {
                        let message = format!("poll failure {}: {}", health.failures(), failure.error);
                        let _ = log::write(&paths.state, &message);
                        if stop {
                            let _ = log::write(&paths.state, "daemon exiting after fatal database loss or 5 consecutive poll failures");
                            break Err(failure.error);
                        }
                    }
                }
            },
            incoming = listener.accept() => match incoming {
                Ok((stream, _)) => { let sender = sender.clone(); connections.spawn(async move { transport::serve(stream, sender).await }); },
                Err(e) => break Err(e),
            },
            Some(call) = receiver.recv() => {
                let stop = call.request.method == "daemon.stop" && methods::empty_params(&call.request);
                let key = key.clone();
                let prepared = methods::prepare(&call.request);
                let response = store.with(move |store| methods::handle_prepared(call.request, store, &key, poll_ms, prepared)).await?;
                let _ = call.reply.send(response);
                if stop { break Ok(()); }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => (),
        }
    };
    drop(polling);
    drop(listener);
    // Let the stop response reach its client, then close any idle connections.
    let _ = tokio::time::timeout(Duration::from_millis(100), async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    connections.shutdown().await;
    store.close().await?;
    result
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
