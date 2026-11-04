use std::{
    fs, io,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::Path,
};
fn identity(socket: &Path) -> io::Result<(u64, u64, i64, i64)> {
    let metadata = fs::metadata(socket)?;
    if !metadata.file_type().is_socket() {
        return Err(io::Error::other("tmux context is not a socket"));
    }
    Ok((
        metadata.dev(),
        metadata.ino(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    ))
}
pub fn read(runtime: &Path, socket: &Path) -> io::Result<u64> {
    let bytes = super::files::read(&runtime.join("server_start"))?;
    let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
    let values = text
        .split_whitespace()
        .map(str::parse::<i128>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    let [start, device, inode, changed, nanos] = values.as_slice() else {
        return Err(io::Error::other("invalid cached server context"));
    };
    let (dev, ino, time, ns) = identity(socket)?;
    if *start <= 0
        || *start > i64::MAX as i128
        || (*device, *inode, *changed, *nanos)
            != (dev as i128, ino as i128, time as i128, ns as i128)
    {
        return Err(io::Error::other("stale or invalid cached server context"));
    }
    Ok(*start as u64)
}
pub fn write(runtime: &Path, start: u64, socket: &Path) -> io::Result<()> {
    if read(runtime, socket).ok() == Some(start) {
        return Ok(());
    }
    let (device, inode, changed, nanos) = identity(socket)?;
    let temp = runtime.join("server_start.next");
    use std::io::Write;
    let mut file = super::files::regular(
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true),
        &temp,
    )?;
    write!(file, "{start} {device} {inode} {changed} {nanos}")?;
    fs::rename(temp, runtime.join("server_start"))
}
/// A zero start is a durable frame captured before the first daemon poll.
pub fn resolve(params: &mut serde_json::Value, runtime: &Path, socket: &Path) {
    if params["server_start"] == 0
        && let Ok(start) = read(runtime, socket)
    {
        params["server_start"] = start.into();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    #[test]
    fn replacing_the_tmux_socket_invalidates_cached_context() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("tmux");
        let listener = UnixListener::bind(&socket).unwrap();
        write(root.path(), 123, &socket).unwrap();
        assert_eq!(read(root.path(), &socket).unwrap(), 123);
        // Hold the old filesystem inode so replacement never depends on allocator reuse.
        fs::rename(&socket, root.path().join("old-tmux")).unwrap();
        drop(listener);
        let _replacement = UnixListener::bind(&socket).unwrap();
        assert!(read(root.path(), &socket).is_err());
        write(root.path(), 124, &socket).unwrap();
        assert_eq!(read(root.path(), &socket).unwrap(), 124);
        // Even a matching device/inode cannot hide a changed socket timestamp.
        let (dev, ino, time, nanos) = identity(&socket).unwrap();
        fs::write(
            root.path().join("server_start"),
            format!("124 {dev} {ino} {} {nanos}", time - 1),
        )
        .unwrap();
        assert!(read(root.path(), &socket).is_err());
    }
}
