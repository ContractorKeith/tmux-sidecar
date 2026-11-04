//! Lock-free appenders; replay owns renamed files, never rewrites the live spool.
use super::files;
use crate::paths::Paths;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
};
pub fn append(paths: &Paths, line: &[u8]) -> io::Result<()> {
    append_inner(paths, line, || Ok(()))
}
fn append_inner(
    paths: &Paths,
    line: &[u8],
    after_open: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    crate::paths::prepare(paths)?;
    let path = paths.state.join("spool.ndjson");
    let mut after_open = Some(after_open);
    loop {
        let mut file = files::regular(
            OpenOptions::new().read(true).create(true).append(true),
            &path,
        )?;
        if let Some(callback) = after_open.take() {
            callback()?;
        }
        let length = file.metadata()?.len();
        let mut tail = *b"\n";
        if length > 0 {
            file.read_at(&mut tail, length - 1)?;
        }
        let mut bytes = Vec::with_capacity(line.len() + 1);
        if tail[0] != b'\n' {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(line);
        // One write with O_APPEND: frames cannot interleave with another appender.
        if file.write(&bytes)? != bytes.len() {
            return Err(io::Error::other("short spool append"));
        }
        let written = file.metadata()?;
        match fs::symlink_metadata(&path) {
            Ok(live)
                if live.is_file() && (live.dev(), live.ino()) == (written.dev(), written.ino()) =>
            {
                return Ok(());
            }
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        // Replay may have read/unlinked an already-open inode. Retry the same
        // nonce in the fresh live spool; SQLite receipts deduplicate either copy.
    }
}
/// Recover prior claims first, then atomically claim the live file.
pub fn claim(state: &Path) -> io::Result<Vec<PathBuf>> {
    let mut claims = Vec::new();
    for entry in fs::read_dir(state)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("spool.replaying.")
        {
            claims.push(entry.path());
        }
    }
    claims.sort();
    let path = state.join("spool.ndjson");
    let file = match files::regular(OpenOptions::new().read(true), &path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(claims),
        Err(e) => return Err(e),
    };
    if file.metadata()?.len() == 0 {
        return Ok(claims);
    }
    let claimed = state.join(format!(
        "spool.replaying.{}.{}",
        std::process::id(),
        super::signal::nonce()?
    ));
    fs::rename(&path, &claimed)?;
    // Keep a live file for new appenders and clients inspecting spool health.
    files::regular(OpenOptions::new().create(true).append(true), &path)?;
    claims.push(claimed);
    Ok(claims)
}
pub fn read(state: &Path) -> io::Result<Vec<u8>> {
    match files::read(&state.join("spool.ndjson")) {
        Ok(bytes) => Ok(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}
pub fn read_claim(path: &Path) -> io::Result<Vec<u8>> {
    files::read(path)
}
pub fn acknowledge(claim: &Path) -> io::Result<()> {
    fs::remove_file(claim)
}
pub fn reject(state: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = files::regular(
        OpenOptions::new().create(true).append(true),
        &state.join("spool.rejected"),
    )?;
    file.write_all(bytes)?;
    file.sync_all()
}
/// Test diagnostics must account for claimed data as well as the live spool.
#[cfg(feature = "test-util")]
pub fn drained(state: &Path) -> io::Result<bool> {
    if !read(state)?.is_empty() {
        return Ok(false);
    }
    Ok(!fs::read_dir(state)?.any(|entry| {
        entry.is_ok_and(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("spool.replaying.")
        })
    }))
}
#[cfg(test)]
#[path = "spool_tests.rs"]
mod tests;
