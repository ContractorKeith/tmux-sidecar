//! Bound inherited-descriptor cleanup before exec, including older Linux kernels.
use std::io;
pub(super) fn limit(value: i32) -> i32 {
    value.clamp(3, 65536)
}
pub(super) fn mark_inherited(max_fd: i32, range: io::Result<()>) -> io::Result<()> {
    match range {
        Ok(()) => return Ok(()),
        Err(e) if matches!(e.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) => (),
        Err(e) => return Err(e),
    }
    for fd in 3..max_fd {
        // SAFETY: fcntl takes no pointers; absent descriptors simply return EBADF.
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    #[test]
    fn huge_descriptor_limits_have_a_bounded_fallback() {
        assert!(limit(i32::MAX) <= 65536);
        assert_eq!(limit(10240), 10240);
    }
    #[test]
    fn old_kernel_errors_fall_back_and_mark_open_fds() {
        for errno in [libc::ENOSYS, libc::EINVAL] {
            let file = std::fs::File::open("/dev/null").unwrap();
            let fd = file.as_raw_fd();
            // SAFETY: file owns fd; these fcntl commands take no pointers.
            unsafe {
                assert_eq!(libc::fcntl(fd, libc::F_SETFD, 0), 0);
            }
            mark_inherited(fd + 1, Err(io::Error::from_raw_os_error(errno))).unwrap();
            // SAFETY: file still owns the live fd.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }
        assert_eq!(
            mark_inherited(0, Err(io::Error::from_raw_os_error(libc::EPERM)))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EPERM)
        );
    }
}
