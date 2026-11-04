use std::io;
use tokio::net::UnixStream;

pub fn same_uid(peer: u32, owner: u32) -> bool {
    peer == owner
}

pub fn authorize(stream: &UnixStream) -> io::Result<bool> {
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
    Ok(same_uid(uid, crate::paths::uid()))
}
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_foreign_uid() {
        assert!(super::same_uid(42, 42));
        assert!(!super::same_uid(43, 42));
    }
}
