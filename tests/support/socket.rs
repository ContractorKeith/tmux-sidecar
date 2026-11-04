use std::{io, os::unix::net::UnixStream, path::Path, time::Duration};
/// Unlike std's connect, this also bounds a connect to an orphaned/full backlog.
pub fn connect(path: impl AsRef<Path>) -> io::Result<UnixStream> {
    connect_with_timeout(path.as_ref(), Duration::from_millis(250))
}
fn connect_with_timeout(path: &Path, timeout: Duration) -> io::Result<UnixStream> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let stream = runtime
        .block_on(async {
            tokio::time::timeout(timeout, tokio::net::UnixStream::connect(path))
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("connect to {} exceeded {timeout:?}", path.display()),
                    )
                })?
        })?
        .into_std()?;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

e2e_test!(full_backlog_has_a_connect_deadline, {
    use std::os::fd::AsRawFd;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    // SAFETY: listener owns the fd; reducing the backlog forces the kernel connect wait.
    assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
    // SAFETY: F_GETFD only inspects the descriptor owned by the listener.
    let flags = unsafe { libc::fcntl(listener.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let mut clients = Vec::new();
    let mut saturated = false;
    for _ in 0..16 {
        match connect_with_timeout(&path, Duration::from_millis(30)) {
            Ok(stream) => {
                // SAFETY: the connected stream owns a valid fd.
                let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
                assert_ne!(flags & libc::FD_CLOEXEC, 0);
                clients.push(stream);
            }
            Err(error) => {
                assert!(matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::ConnectionRefused
                ));
                saturated = true;
                break;
            }
        }
    }
    assert!(saturated, "test must saturate the Unix listen backlog");
});
