use super::*;
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::{fs::OpenOptionsExt, process::CommandExt},
    },
    process::{Child, Command, Stdio},
    time::Instant,
};

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
            .custom_flags(libc::O_NOFOLLOW)
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

// Until startup succeeds, cancellation and every error path must reap the child.
struct StartingChild(Option<Child>);
impl Drop for StartingChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) async fn ensure(socket: &Path, paths: &Paths) -> io::Result<()> {
    paths::prepare(paths)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let _lock = loop {
        match FileLock::try_acquire(&paths.runtime.join("spawn.lock")) {
            Ok(lock) => break lock,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    };
    if connect(paths).await.is_ok() {
        return Ok(());
    }
    let pid = fs::read_to_string(paths.runtime.join("daemon.pid"))
        .ok()
        .and_then(|p| p.trim().parse().ok());
    if !pid.is_some_and(crate::proc::alive) {
        match fs::remove_file(paths.runtime.join("sock")) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(paths.state.join("daemon.log"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--tmux-socket")
        .arg(socket)
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // SAFETY: pre_exec calls only async-signal-safe setsid and constructs an OS error on failure.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut starting = StartingChild(Some(command.spawn()?));
    loop {
        if connect(paths).await.is_ok() {
            // Reap when the caller stays alive (library users); the detached daemon otherwise
            // outlives this CLI and is adopted by init.
            let mut child = starting.0.take().expect("startup child is present");
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        if let Some(status) = starting
            .0
            .as_mut()
            .expect("startup child is present")
            .try_wait()?
        {
            return Err(io::Error::other(format!(
                "daemon exited {status}; see daemon.log"
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
