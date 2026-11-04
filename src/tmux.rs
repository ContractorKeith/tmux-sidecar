//! M0: pane metadata adapter; mutations arrive in M4/M5.
use crate::seams::{PaneSnapshot, Tmux};
use std::{io, path::PathBuf, process::Command};

// Length framing preserves tabs/newlines in cwd and command (session-board's idiom).
const FORMAT: &str = concat!(
    "#{n:start_time}:#{start_time}",
    "#{n:pane_id}:#{pane_id}",
    "#{n:pane_pid}:#{pane_pid}",
    "#{n:pane_current_path}:#{pane_current_path}",
    "#{n:pane_current_command}:#{pane_current_command}",
    "#{n:pane_active}:#{pane_active}",
    "#{n:window_active}:#{window_active}",
    "#{n:session_attached}:#{session_attached}",
);

pub enum Socket {
    Path(PathBuf),
    Name(String),
}
pub struct RealTmux {
    pub socket: Socket,
}
impl RealTmux {
    pub fn confirmed_gone(&self, first: &io::Error) -> bool {
        let Socket::Path(path) = &self.socket else {
            return false;
        };
        confirm_gone(first, || path.exists(), || self.scan())
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new("tmux");
        match &self.socket {
            Socket::Path(path) => {
                command.arg("-S").arg(path);
            }
            Socket::Name(name) => {
                command.arg("-L").arg(name);
            }
        }
        command
    }
}
impl Tmux for RealTmux {
    fn list_panes(&self) -> io::Result<Vec<PaneSnapshot>> {
        Ok(self.scan()?.panes)
    }
    fn scan(&self) -> io::Result<PaneScan> {
        let output = self
            .command()
            .args(["list-panes", "-a", "-F", FORMAT])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        parse_scan(&output.stdout)
    }
    fn paste_text(&self, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported("M4"))
    }
    fn send_enter(&self, _: &str) -> io::Result<()> {
        Err(unsupported("M4"))
    }
    fn display_message(&self, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported("M5"))
    }
}
fn unsupported(milestone: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("not implemented yet ({milestone})"),
    )
}
/// Used only when neither an explicit socket nor TMUX selects a server.
pub fn default_socket() -> io::Result<PathBuf> {
    let output = Command::new("tmux")
        .args(["display", "-p", "#{socket_path}"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "no tmux server; start tmux or select one with --tmux-socket",
        ));
    }
    let path = String::from_utf8(output.stdout).map_err(io::Error::other)?;
    if path.trim().is_empty() {
        return Err(io::Error::other("tmux returned an empty socket path"));
    }
    Ok(PathBuf::from(path.trim_end()))
}

fn confirm_gone(
    first: &io::Error,
    exists: impl Fn() -> bool,
    probe: impl FnOnce() -> io::Result<PaneScan>,
) -> bool {
    if !gone_evidence(first, exists()) {
        return false;
    }
    match probe() {
        Ok(_) => false,
        Err(second) => gone_evidence(&second, exists()),
    }
}
fn gone_evidence(error: &io::Error, socket_exists: bool) -> bool {
    !socket_exists
        || error.to_string().contains("no server running")
        || error.to_string().contains("error connecting to")
}
mod parse;
pub use parse::{PaneScan, parse, parse_scan};
#[cfg(test)]
#[path = "tmux/tests.rs"]
mod tests;
