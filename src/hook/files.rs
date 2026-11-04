//! Hook-owned files must never follow links or block on a FIFO.
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
pub fn regular(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    let file = options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("hook file is not regular"));
    }
    Ok(file)
}
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    regular(OpenOptions::new().read(true), path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}
