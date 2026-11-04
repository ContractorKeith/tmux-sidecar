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
    let stat = match std::fs::read(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
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
