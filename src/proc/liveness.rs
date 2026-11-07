use std::io;
/// A zombie has exited even while its pid still responds to signal 0.
pub fn running(pid: i32) -> io::Result<bool> {
    if !super::alive(pid) {
        return Ok(false);
    }
    state_running(pid)
}
#[cfg(target_os = "linux")]
fn state_running(pid: i32) -> io::Result<bool> {
    linux_stat(std::fs::read(format!("/proc/{pid}/stat")))
}
#[cfg(target_os = "linux")]
fn linux_stat(result: io::Result<Vec<u8>>) -> io::Result<bool> {
    let stat = match result {
        Ok(stat) => stat,
        // A process can disappear after kill(0), or after opening its proc file.
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(libc::ESRCH) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let end = stat
        .windows(2)
        .rposition(|bytes| bytes == b") ")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat record"))?;
    Ok(stat.get(end + 2) != Some(&b'Z'))
}
#[cfg(target_os = "macos")]
fn state_running(pid: i32) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: info points to a writable proc_bsdinfo-sized buffer; libproc writes at most size bytes.
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count == size {
        // SAFETY: a full-size successful PROC_PIDTBSDINFO response initializes the entire structure.
        return Ok(unsafe { info.assume_init() }.pbi_status != libc::SZOMB);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) || !super::alive(pid) {
        return Ok(false);
    }
    Err(io::Error::new(
        error.kind(),
        format!("query exit state of pid {pid}: {error}"),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn process_exit_between_probe_and_stat_read_counts_as_exited() {
        for code in [libc::ENOENT, libc::ESRCH] {
            assert!(!linux_stat(Err(io::Error::from_raw_os_error(code))).unwrap());
        }
        assert_eq!(
            linux_stat(Err(io::Error::from_raw_os_error(libc::EACCES)))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EACCES)
        );
    }
}
