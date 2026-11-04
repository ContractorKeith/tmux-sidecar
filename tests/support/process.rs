use std::{
    io::{self, Read},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn reader(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<io::Result<Vec<u8>>> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe.read_to_end(&mut bytes).map(|_| bytes);
        let _ = send.send(result);
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
    let mut child = ChildGuard(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = reader(child.0.stdout.take().unwrap());
    let stderr = reader(child.0.stderr.take().unwrap());
    let deadline = Instant::now() + timeout;
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
    let receive = |stream: &str, receiver: mpsc::Receiver<io::Result<Vec<u8>>>| {
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{stream} EOF timed out after child exit ({status}): {error}"),
                )
            })?
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

#[test]
fn timeout_kills_and_reaps_the_owned_process() {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new("/bin/sleep");
    command
        .arg("30")
        .env("HOME", root.path())
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.path().join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.path().join("state"));
    let start = Instant::now();
    let error = output_with_timeout(&mut command, Duration::from_millis(20)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(2));
}

// Direct writes remain visible if the test harness is waiting on a hung child.
pub fn stage(message: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{message}");
}
