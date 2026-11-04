use crate::{client::FileLock, paths::Paths};
use std::{
    io,
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};
/// Daemon lifetime locking arbitrates concurrent triggers; the hook never waits for startup.
pub fn trigger_spawn(socket: &Path, paths: &Paths) -> io::Result<()> {
    let _lock = FileLock::try_acquire(&paths.runtime.join("spawn.lock"))?;
    let log = crate::daemon::log::open(&paths.state)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--tmux-socket")
        .arg(socket)
        .args(["daemon", "run"])
        .env("TMUX_SIDECAR_DETACHED", "1")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // SAFETY: setsid is async-signal-safe and receives no pointers.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
