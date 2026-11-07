//! Subscription-driven terminal client. All run data comes from the daemon socket.
mod input;
mod render;
mod stream;
#[cfg(test)]
mod tests;
use crate::paths::Paths;
mod app;
pub use app::{App, Group, Row, Run, Snapshot};
use crossterm::{event, execute, terminal};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, path::PathBuf, time::Duration};

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}
struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn run(socket: PathBuf, paths: Paths) -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
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
    let mut tick = tokio::time::interval(Duration::from_millis(30));
    loop {
        tokio::select! {
            changed=receive.changed()=> {if changed.is_err(){return Err(io::Error::other("UI connection worker stopped"));} app.replace(receive.borrow_and_update().clone());},
            _=tick.tick()=> {
                if event::poll(Duration::ZERO)? {
                    let action=match event::read()? {event::Event::Key(key)=>input::decode(&mut app,key),_=>input::Action::None};
                    match action {
                        input::Action::Quit=>return Ok(()),
                        input::Action::Open=>if let Some(run)=app.selected_run() {
                            let pane=run.run.pane_id.clone(); let start=run.run.server_start;
                            match crate::tmux::focus(&socket,&pane,start).await {Ok(())=>return Ok(()),Err(e)=>app.message=e.to_string()}
                        },
                        input::Action::Ack=>if let Some(run)=app.selected_run() {
                            let result=crate::client::request(&socket,&paths,"ack",serde_json::json!({"sel":run.run.id}),false).await;
                            if let Err(e)=result {app.message=e.to_string();}
                        },
                        input::Action::None=>(),
                    }
                }
            }
        }
        terminal.draw(|frame| render::draw(&app, frame, crate::daemon::now()))?;
    }
}
