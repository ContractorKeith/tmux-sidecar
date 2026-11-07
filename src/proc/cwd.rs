use std::{io, path::PathBuf};
#[cfg(target_os = "linux")]
pub(super) fn read(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd"))
}
#[cfg(target_os = "macos")]
pub(super) fn read(pid: u32) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    // SAFETY: libproc writes at most size bytes to the owned buffer and retains no pointer.
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if count != size {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful full-size response initialized the entire struct.
    let info = unsafe { info.assume_init() };
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .take_while(|&&b| b != 0)
        .map(|&b| b as u8)
        .collect();
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
}
#[cfg(test)]
mod tests {
    use crate::seams::ProcessTree;
    #[test]
    fn process_cwd_is_available_through_the_process_tree_seam() {
        assert_eq!(
            crate::proc::PsProcessTree.cwd(std::process::id()).unwrap(),
            std::env::current_dir().unwrap()
        );
        assert!(crate::proc::PsProcessTree.cwd(u32::MAX).is_err());
    }
}
