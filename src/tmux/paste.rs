//! Named buffers keep prompts out of tmux's key parser and other clients' buffers.
use super::RealTmux;
use std::{
    io::{self, Write},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn run(command: &mut Command, input: Option<&str>, whole: Instant) -> io::Result<()> {
    let deadline = whole.min(Instant::now() + Duration::from_secs(1));
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "send deadline exceeded before tmux command",
        ));
    }
    let mut child = ChildGuard(
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    if let Some(input) = input {
        use std::os::fd::AsRawFd;
        let mut pipe = child.0.stdin.take().expect("piped load-buffer stdin");
        // SAFETY: pipe owns this valid fd; F_GETFL/F_SETFL do not use pointers.
        unsafe {
            let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
            if flags < 0
                || libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut bytes = input.as_bytes();
        while !bytes.is_empty() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "tmux load-buffer stdin exceeded 1 s",
                ));
            }
            match pipe.write(bytes) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "tmux load-buffer closed stdin",
                    ));
                }
                Ok(n) => bytes = &bytes[n..],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e),
            }
        }
    }
    loop {
        if let Some(status) = child.0.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("tmux exited {status}")))
            };
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "tmux mutation exceeded 1 s",
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub fn paste(tmux: &RealTmux, pane: &str, text: &str) -> io::Result<()> {
    paste_before(tmux, pane, text, Instant::now() + Duration::from_secs(3))
}
pub fn paste_before(tmux: &RealTmux, pane: &str, text: &str, deadline: Instant) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let buffer = format!(
        "tmux-sidecar-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let result = run(
        tmux.command().args(["load-buffer", "-b", &buffer, "-"]),
        Some(text),
        deadline,
    )
    .and_then(|()| {
        run(
            tmux.command()
                .args(["paste-buffer", "-p", "-d", "-r", "-b", &buffer, "-t", pane]),
            None,
            deadline,
        )
    });
    if result.is_err() {
        let _ = run(
            tmux.command().args(["delete-buffer", "-b", &buffer]),
            None,
            deadline,
        );
    }
    result
}
pub fn enter(tmux: &RealTmux, pane: &str) -> io::Result<()> {
    enter_before(tmux, pane, Instant::now() + Duration::from_secs(1))
}
pub fn enter_before(tmux: &RealTmux, pane: &str, deadline: Instant) -> io::Result<()> {
    run(
        tmux.command().args(["send-keys", "-t", pane, "Enter"]),
        None,
        deadline,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_send_never_starts_a_terminal_command() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("late");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf late > \"$TSX_MARKER\""])
            .env("TSX_MARKER", &marker);
        let error = run(&mut command, None, Instant::now() - Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(!marker.exists());
    }
}
