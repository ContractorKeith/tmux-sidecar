use crate::{client::FileLock, paths::Paths};
use std::{
    io,
    path::Path,
    process::{Command, Stdio},
};
/// Daemon lifetime locking arbitrates concurrent triggers; the hook never waits for startup.
pub fn trigger_spawn(socket: &Path, paths: &Paths) -> io::Result<()> {
    let _lock = FileLock::try_acquire(&paths.state.join("spawn.lock"))?;
    let log = crate::daemon::log::open(&paths.state)?;
    let mut command = Command::new(std::env::current_exe()?);
    super::super::launch::prepare(&mut command)?;
    command
        .arg("--tmux-socket")
        .arg(socket)
        .args(["daemon", "run"])
        .env("TMUX_SIDECAR_DETACHED", "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    match crate::tmux::for_spawn(paths) {
        Ok(binary) => {
            command.env("TMUX_SIDECAR_TMUX", binary);
        }
        Err(_) => {
            command.env_remove("TMUX_SIDECAR_TMUX");
        }
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
