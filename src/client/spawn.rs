use super::*;
#[path = "trigger.rs"]
mod trigger;
use std::{
    fs::{self, File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    process::Stdio,
    time::Instant,
};
use tokio::process::{Child, Command};
pub use trigger::trigger_spawn;

/// Locks live as long as their file; CLOEXEC prevents a spawned daemon inheriting them.
pub struct FileLock(File);
impl FileLock {
    pub fn try_acquire(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        // SAFETY: file owns a valid descriptor, flock takes no pointers.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: the file remains valid until after drop returns.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

// Binding the socket hands lifecycle ownership to the daemon. Cancellation before
// that point sends SIGTERM and reaps asynchronously, allowing normal cleanup.
struct StartingChild {
    child: Option<Child>,
    socket: PathBuf,
}
impl StartingChild {
    fn release(&mut self) {
        // Tokio's Unix orphan queue reaps a dropped Child after SIGCHLD. There is
        // no waiting thread, and kill_on_drop is false for a handed-over daemon.
        drop(self.child.take());
    }
}
impl Drop for StartingChild {
    fn drop(&mut self) {
        if !self.socket.exists()
            && let Some(child) = &self.child
            && let Some(pid) = child.id()
        {
            crate::proc::terminate(pid);
        }
        self.release();
    }
}

pub(super) async fn ensure(socket: &Path, paths: &Paths) -> io::Result<()> {
    paths::prepare(paths)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let _lock = loop {
        match FileLock::try_acquire(&paths.state.join("spawn.lock")) {
            Ok(lock) => break lock,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "waiting for {} exceeded 2 s",
                        paths.state.join("spawn.lock").display()
                    ),
                ));
            }
            Err(e) => return Err(e),
        }
    };
    if connect(paths).await.is_ok() {
        return Ok(());
    }
    let log = crate::daemon::log::open(&paths.state)?;
    let mut command = Command::new(std::env::current_exe()?);
    super::launch::prepare(command.as_std_mut())?;
    command
        .arg("--tmux-socket")
        .arg(socket)
        .env("TMUX_SIDECAR_DETACHED", "1")
        .env("TMUX_SIDECAR_TMUX", crate::tmux::for_spawn(paths)?)
        .args(["daemon", "run"])
        .kill_on_drop(false)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    let mut starting = StartingChild {
        child: Some(command.spawn()?),
        socket: paths.runtime.join("sock"),
    };
    loop {
        if starting.socket.exists() {
            starting.release();
        }
        if connect(paths).await.is_ok() {
            starting.release();
            return Ok(());
        }
        if let Some(child) = &mut starting.child
            && let Some(status) = child.try_wait()?
        {
            // A detached hook trigger can win the lifetime lock after our connect.
            // Join that startup rather than reporting the losing child's lock error.
            if FileLock::try_acquire(&paths.state.join("daemon.lock"))
                .is_err_and(|e| e.kind() == io::ErrorKind::WouldBlock)
            {
                starting.release();
                continue;
            }
            let log = paths.state.join("daemon.log");
            let detail = fs::read_to_string(&log)
                .ok()
                .and_then(|s| {
                    s.lines()
                        .rev()
                        .find(|line| !line.trim().is_empty())
                        .map(|line| line.trim().to_owned())
                })
                .unwrap_or_else(|| "daemon failed before reporting a reason".into());
            return Err(io::Error::other(format!(
                "daemon exited {status}: {detail}; see {}",
                log.display()
            )));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "daemon did not start within 2 s",
            ));
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
#[path = "spawn_tests.rs"]
mod tests;
