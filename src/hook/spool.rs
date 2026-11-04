use super::files;
use crate::{client::FileLock, paths::Paths};
use std::os::unix::fs::FileExt;
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    time::{Duration, Instant},
};

fn lock(state: &Path) -> io::Result<FileLock> {
    let deadline = Instant::now() + Duration::from_millis(15);
    loop {
        match FileLock::try_acquire(&state.join("spool.lock")) {
            Ok(lock) => return Ok(lock),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) => return Err(e),
        }
    }
}
pub fn append(paths: &Paths, line: &[u8]) -> io::Result<()> {
    crate::paths::prepare(paths)?;
    let _lock = lock(&paths.state)?;
    let mut file = files::regular(
        OpenOptions::new().read(true).create(true).append(true),
        &paths.state.join("spool.ndjson"),
    )?;
    let length = file.metadata()?.len();
    let mut tail = *b"\n";
    if length > 0 {
        file.read_at(&mut tail, length - 1)?;
    }
    let line = if tail[0] != b'\n' {
        let mut separated = vec![b'\n'];
        separated.extend_from_slice(line);
        separated
    } else {
        line.to_vec()
    };
    if file.write(&line)? != line.len() {
        return Err(io::Error::other("short spool append"));
    }
    Ok(())
}

pub fn read(state: &Path) -> io::Result<Vec<u8>> {
    let _lock = lock(state)?;
    match files::read(&state.join("spool.ndjson")) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}
/// Remove only the committed prefix; hooks may have appended while SQLite was busy.
pub fn acknowledge(state: &Path, prefix: &[u8]) -> io::Result<()> {
    let _lock = lock(state)?;
    let path = state.join("spool.ndjson");
    let mut file = files::regular(OpenOptions::new().read(true).write(true), &path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if !bytes.starts_with(prefix) {
        return Err(io::Error::other("spool prefix changed"));
    }
    let remaining = &bytes[prefix.len()..];
    // Atomic replacement prevents a crash during truncation from corrupting a suffix.
    let temp = state.join("spool.replay");
    let mut replacement = files::regular(
        OpenOptions::new().create(true).truncate(true).write(true),
        &temp,
    )?;
    replacement.write_all(remaining)?;
    replacement.sync_all()?;
    fs::rename(temp, path)
}

pub fn reject(state: &Path, bytes: &[u8]) -> io::Result<()> {
    let _lock = lock(state)?;
    let mut file = files::regular(
        OpenOptions::new().create(true).append(true),
        &state.join("spool.rejected"),
    )?;
    file.write_all(bytes)?;
    file.sync_all()
}
