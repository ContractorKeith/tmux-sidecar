//! Subscription-driven terminal client. All run data comes from the daemon socket.
mod app;
mod input;
mod render;
mod stream;
mod terminal;
#[cfg(test)]
mod tests;
use crate::paths::Paths;
pub use app::{App, Group, Row, Run, Snapshot};
use crossterm::event::{Event, EventStream};
use futures_core::Stream;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, path::PathBuf, time::Duration};
struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn action(
    app: &mut App,
    action: input::Action,
    socket: &std::path::Path,
    paths: &Paths,
) -> io::Result<bool> {
    match action {
        input::Action::Quit => return Ok(true),
        input::Action::Open => {
            if let Some(run) = app.selected_run() {
                match crate::tmux::focus(socket, &run.run.pane_id, run.run.server_start).await {
                    Ok(()) => return Ok(true),
                    Err(e) => app.message = e.to_string(),
                }
            }
        }
        input::Action::Ack => {
            if let Some(run) = app.selected_run()
                && let Err(e) = crate::client::request(
                    socket,
                    paths,
                    "ack",
                    serde_json::json!({"sel":run.run.id}),
                    false,
                )
                .await
            {
                app.message = e.to_string();
            }
        }
        input::Action::None | input::Action::Ignored => (),
    }
    Ok(false)
}
pub async fn run(socket: PathBuf, paths: Paths) -> io::Result<()> {
    // Register before entering raw mode so termination always returns through Restore.
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    let mut hup = signal(SignalKind::hangup())?;
    let _restore = terminal::Restore::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let (send, mut receive) = tokio::sync::watch::channel(Snapshot::default());
    let net_socket = socket.clone();
    let net_paths = crate::paths::current(&socket)?;
    let _network = Task(tokio::spawn(async move {
        stream::run(net_socket, net_paths, send).await
    }));
    let mut app = App {
        color: std::env::var_os("NO_COLOR").is_none(),
        ..App::default()
    };
    let mut ages = tokio::time::interval(Duration::from_secs(1));
    ages.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // First-frame readiness precedes input initialization and daemon I/O.
    terminal.draw(|frame| render::draw(&app, frame, crate::daemon::now()))?;
    let mut events = EventStream::new();
    loop {
        tokio::select! {
            _=term.recv()=>return Ok(()),
            _=hup.recv()=>return Ok(()),
            changed=receive.changed()=>{
                changed.map_err(|_|io::Error::other("UI connection worker stopped"))?;
                app.replace(receive.borrow_and_update().clone());
            },
            event=std::future::poll_fn(|cx|std::pin::Pin::new(&mut events).poll_next(cx))=>match event {
                Some(Ok(Event::Key(key)))=>{
                    let decoded=input::decode(&mut app,key);
                    if decoded==input::Action::Ignored {continue;}
                    if action(&mut app,decoded,&socket,&paths).await? {return Ok(());}
                },
                Some(Ok(Event::Resize(..)))=>(),
                Some(Err(e))=>return Err(e),
                None=>return Ok(()),
                _=>continue,
            },
            _=ages.tick()=>(),
        }
        terminal.draw(|frame| render::draw(&app, frame, crate::daemon::now()))?;
    }
}
