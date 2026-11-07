//! One socket owner and one poller per tmux server.
mod actions;
mod config;
mod labels;
mod list;
pub mod log;
mod methods;
pub(crate) mod recovery;
mod state_methods;
mod transcript_match;
mod transcripts;
pub use methods::handle as dispatch;
mod peer;
mod poll;
mod replay;
mod shutdown;
mod signal_delivery;
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

use shutdown::{NotificationTask, RuntimeGuard};

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
    let recreated = recovery::was_deleted(&paths.state, &db)?;
    if !db.exists() && !recreated {
        log::write(
            &paths.state,
            &format!("creating fresh database {}", db.display()),
        )?;
    }
    let store = store_worker::StoreWorker::open(db).await?;
    recovery::created(paths, recreated)?;
    let notify_config = crate::notify::Config::read(&paths.config)?;
    let notifications = crate::notify::start(
        socket.clone(),
        paths.state.clone(),
        notify_config,
        store.notifications.subscribe(),
    );
    let _notifications = NotificationTask(notifications);
    let poll_ms = config::poll_ms(paths)?;
    let idle_after_ms = config::idle_after_ms(paths)?;
    let send_confirm_ms = config::send_confirm_ms(paths)?;
    let mut transcripts = transcripts::Reader::default();
    let mut health = poll::Health::default();
    let initial = poll::tick(
        socket.clone(),
        &store,
        paths.runtime.clone(),
        idle_after_ms,
        send_confirm_ms,
    )
    .await;
    health.record(&initial);
    match initial {
        Ok(false) => return Err(io::Error::other("tmux server is not running")),
        Err(f) if f.fatal => return Err(f.error),
        Err(f) => {
            log::write(&paths.state, &format!("initial poll failure: {}", f.error))?;
        }
        Ok(true) => (),
    }
    replay::replay(
        paths.state.clone(),
        paths.runtime.clone(),
        socket.clone(),
        &store,
    )
    .await;
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
    let mut transcript_interval = tokio::time::interval(Duration::from_secs(2));
    transcript_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut polling: Option<PollFuture<'_>> = None;
    let result = loop {
        tokio::select! {
            _ = term.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
            _ = transcript_interval.tick() => transcripts.start(store.clone(),paths.state.clone()),
            _ = interval.tick(), if polling.is_none() => {
                let socket = socket.clone();
                let state = paths.state.clone();
                let worker = &store;
                let runtime = paths.runtime.clone();
                polling = Some(Box::pin(async move {
                    replay::replay(state, runtime.clone(),socket.clone(), worker).await;
                    let alive = poll::tick(socket, worker, runtime, idle_after_ms, send_confirm_ms).await?;
                    Ok(alive)
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
                Ok((stream, _)) => { let sender = sender.clone(); let events=store.events.subscribe(); let views=store.views.subscribe(); connections.spawn(async move { transport::serve(stream, sender, events, views).await }); },
                Err(e) => break Err(e),
            },
            Some(mut call) = receiver.recv() => {
                if call.request.method=="signal" && let Some(params)=&mut call.request.params {crate::hook::context::resolve(params,&paths.runtime,&socket);}
                if matches!(call.request.method.as_str(), "list" | "attention") {
                    let worker=store.clone();
                    connections.spawn(async move {list::serve(call,worker).await;Ok(())});
                    continue;
                }
                if call.request.method == "signal" {
                    let worker=store.clone();let state=paths.state.clone();
                    connections.spawn(async move {
                        let response=signal_delivery::respond(call.request,&worker,state).await;
                        let _=call.reply.send(response);Ok(())
                    });continue;
                }
                let stop = call.request.method == "daemon.stop" && methods::empty_params(&call.request);
                let key = key.clone();
                let prepared = methods::prepare(&call.request);
                let server_start=crate::hook::context::read(&paths.runtime,&socket).unwrap_or(0);
                if matches!(call.request.method.as_str(), "wait" | "send") {
                    let worker = store.clone();
                    let socket = socket.clone();
                    connections.spawn(async move { actions::serve(call, worker, socket, server_start).await; Ok(()) });
                    continue;
                }
                if stop {
                    let response = store.read(move |store| methods::handle_prepared(call.request, store, &key, poll_ms, prepared,server_start)).await?;
                    let _ = call.reply.send(response);
                } else {
                    let worker = store.clone();
                    connections.spawn(async move {
                        let job = move |store: &crate::store::Store| methods::handle_prepared(call.request, store, &key, poll_ms, prepared,server_start);
                        let response = worker.read(job).await?;
                        let _ = call.reply.send(response);
                        Ok(())
                    });
                }
                if stop { break Ok(()); }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => (),
        }
    };
    transcripts.stop().await;
    drop(polling);
    drop(listener);
    let drained =
        shutdown::drain(&mut connections, &mut receiver, &store, paths.state.clone()).await;
    store.close().await?;
    result.and(drained)
}

#[cfg(test)]
mod transport_tests;
