#[path = "../isolation/mod.rs"]
mod isolation;
use std::{
    io::{self, Read},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct ChildGuard(pub Child);
impl ChildGuard {
    pub fn new(child: Child) -> Self {
        super::watchdog::track(child.id());
        Self(child)
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        super::watchdog::untrack(self.0.id());
        let _ = self.0.kill();
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.0.try_wait().ok().flatten().is_none() {
            if Instant::now() >= deadline {
                let message = format!("reap child pid {} after kill exceeded 1 s", self.0.id());
                if std::thread::panicking() {
                    stage(&message);
                } else {
                    panic!("{message}");
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
pub trait BoundedCommand {
    fn bounded_output(&mut self) -> io::Result<Output>;
}
impl BoundedCommand for Command {
    fn bounded_output(&mut self) -> io::Result<Output> {
        try_output(self)
    }
}
pub fn wait_for(name: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(Instant::now() < deadline, "wait for {name} exceeded 4 s");
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn reader(
    mut pipe: impl Read + std::os::fd::AsRawFd + Send + 'static,
    deadline: Instant,
    name: &'static str,
) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut read = || -> io::Result<Vec<u8>> {
            // SAFETY: the owned pipe fd is valid; only this thread reads it.
            let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
            if flags < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: F_SETFL changes this live pipe's flags, with no pointers.
            let result =
                unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
            if result < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("{name} pipe EOF deadline expired"),
                    ));
                }
                match pipe.read(&mut buffer) {
                    Ok(0) => return Ok(bytes),
                    Ok(n) => bytes.extend_from_slice(&buffer[..n]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                    Err(error) => return Err(error),
                }
            }
        };
        let _ = send.send(read());
    });
    receive
}
/// Bound both process exit and pipe EOF: a detached descendant must not hold up CI.
pub fn output(command: &mut Command) -> Output {
    try_output(command)
        .unwrap_or_else(|error| panic!("{command:?}: {error}; {}", diagnostics(command)))
}
pub fn try_output(command: &mut Command) -> io::Result<Output> {
    output_with_timeout(command, Duration::from_secs(8))
}
fn output_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    let sandbox = isolation::fill(command);
    let _registration = sandbox
        .as_ref()
        .map(|root| super::watchdog::Registration::new(root.path()));
    let mut child = ChildGuard::new(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let deadline = Instant::now() + timeout;
    let stdout = reader(child.0.stdout.take().unwrap(), deadline, "stdout");
    let stderr = reader(child.0.stderr.take().unwrap(), deadline, "stderr");
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("process exit timed out: pid {}", child.0.id()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let pid = child.0.id();
    let receive = |stream: &str, receiver: mpsc::Receiver<io::Result<Vec<u8>>>| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{stream} EOF timed out after child exit ({status}): {error}"),
                )
            })?
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("{stream} EOF after child pid {pid} exited {status}: {error}"),
                )
            })
    };
    Ok(Output {
        status,
        stdout: receive("stdout", stdout)?,
        stderr: receive("stderr", stderr)?,
    })
}

fn diagnostics(command: &Command) -> String {
    let state = command
        .get_envs()
        .find(|(key, _)| *key == "TMUX_SIDECAR_STATE_DIR")
        .and_then(|(_, value)| value);
    let runtime = command
        .get_envs()
        .find(|(key, _)| *key == "TMUX_SIDECAR_RUNTIME_DIR")
        .and_then(|(_, value)| value);
    let log = state
        .and_then(|path| {
            std::fs::read_to_string(std::path::Path::new(path).join("daemon.log")).ok()
        })
        .unwrap_or_default();
    let pid = runtime
        .and_then(|path| {
            std::fs::read_to_string(std::path::Path::new(path).join("daemon.pid")).ok()
        })
        .unwrap_or_default();
    format!("daemon pid={pid}, log={log}")
}

e2e_test!(timeout_kills_and_reaps_the_owned_process, {
    let root = crate::support::Scratch::new();
    let mut command = Command::new("/bin/sleep");
    command
        .arg("30")
        .env("HOME", root.path())
        .env("TMUX_TMPDIR", root.path())
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"));
    let start = Instant::now();
    let error = output_with_timeout(&mut command, Duration::from_millis(20)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(2));
});

// Direct writes remain visible if the test harness is waiting on a hung child.
pub fn stage(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}
