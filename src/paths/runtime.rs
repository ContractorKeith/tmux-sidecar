use super::{Paths, Platform, resolve, server_key};
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
};

pub fn uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory requirements.
    unsafe { libc::geteuid() }
}

pub fn current(socket: &Path) -> io::Result<Paths> {
    let home = std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| io::Error::other("HOME is not set"))?;
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Linux
    };
    let paths = resolve(
        platform,
        Path::new(&home),
        uid(),
        &server_key(socket)?,
        |name| std::env::var_os(name),
    );
    match verify_dir(&paths.runtime, uid()) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    Ok(paths)
}

/// Refuse symlinks and insecure existing directories; never repair an untrusted runtime.
pub fn prepare(paths: &Paths) -> io::Result<()> {
    secure_dir(&paths.runtime, uid())?;
    secure_dir(&paths.state, uid())
}

fn secure_dir(path: &Path, owner: u32) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    verify_dir(path, owner)
}

fn verify_dir(path: &Path, owner: u32) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is a symlink", path.display()),
        ));
    }
    if !meta.is_dir() || meta.uid() != owner || meta.mode() & 0o7777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be owned by uid {owner} with mode 0700",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn creates_private_dirs_and_refuses_insecure_or_foreign_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("run");
        secure_dir(&path, uid()).unwrap();
        secure_dir(&path, uid()).unwrap();
        assert!(secure_dir(&path, uid().wrapping_add(1)).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(secure_dir(&path, uid()).is_err());
        let link = tmp.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(secure_dir(&link, uid()).is_err());
    }
}
