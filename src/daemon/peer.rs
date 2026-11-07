use std::io;
use tokio::net::UnixStream;

pub fn same_uid(peer: u32, owner: u32) -> bool {
    peer == owner
}

pub fn uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    let uid = stream.peer_cred()?.uid(); // tokio uses SO_PEERCRED.
    #[cfg(target_os = "macos")]
    let uid = {
        use std::os::fd::AsRawFd;
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: descriptor is live; uid/gid point to writable initialized integers.
        if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        uid
    };
    Ok(uid)
}
/// EOF alone is a write-half close; POLLHUP identifies a fully disconnected peer.
pub fn disconnected(stream: &UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    let mut descriptor = libc::pollfd {
        fd: stream.as_raw_fd(),
        // Darwin poll registers only requested filters; request write readiness
        // to observe a fully closed peer while preserving write-half closes.
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: descriptor is initialized, writable, and backed by the live stream.
    unsafe { libc::poll(&mut descriptor, 1, 0) > 0 && descriptor.revents & libc::POLLHUP != 0 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_foreign_uid() {
        assert!(super::same_uid(42, 42));
        assert!(!super::same_uid(43, 42));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn listener_and_peer_descriptors_close_on_exec() {
        use std::os::fd::AsRawFd;
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::UnixListener::bind(root.path().join("sock")).unwrap();
        let (client, server) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            let client = tokio::net::UnixStream::connect(root.path().join("sock"))
                .await
                .unwrap();
            let (server, _) = listener.accept().await.unwrap();
            (client, server)
        })
        .await
        .expect("connect and accept peer descriptors within 1 s");
        assert_eq!(super::uid(&server).unwrap(), crate::paths::uid());
        for fd in [listener.as_raw_fd(), client.as_raw_fd(), server.as_raw_fd()] {
            // SAFETY: listener/client/server own every descriptor throughout this loop.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(
                flags & libc::FD_CLOEXEC,
                0,
                "peer/listener fd must not survive exec"
            );
        }
    }
}
