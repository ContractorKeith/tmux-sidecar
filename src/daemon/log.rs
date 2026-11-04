//! Bounded daemon diagnostics; each write reopens after rotation.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};
const LIMIT: u64 = 1024 * 1024;
pub fn open(state: &Path) -> io::Result<std::fs::File> {
    rotate(state, 0)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(state.join("daemon.log"))
}
fn rotate(state: &Path, incoming: u64) -> io::Result<()> {
    let path = state.join("daemon.log");
    match fs::metadata(&path) {
        Ok(meta) if meta.len().saturating_add(incoming) >= LIMIT => {
            fs::rename(path, state.join("daemon.log.1"))
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
pub fn write(state: &Path, message: &str) -> io::Result<()> {
    let mut bytes = message.as_bytes();
    if bytes.len() >= LIMIT as usize {
        bytes = &bytes[..LIMIT as usize - 1];
    }
    rotate(state, bytes.len() as u64 + 1)?;
    let mut file = open(state)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_log_with_one_rotation() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..4 {
            write(dir.path(), &"x".repeat(700_000)).unwrap();
        }
        for name in ["daemon.log", "daemon.log.1"] {
            assert!(fs::metadata(dir.path().join(name)).unwrap().len() <= LIMIT);
        }
        assert!(!dir.path().join("daemon.log.2").exists());
    }
}
