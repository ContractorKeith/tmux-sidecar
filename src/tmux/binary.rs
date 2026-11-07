use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
static RETRY_PATH: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicBool = AtomicBool::new(false);
fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn resolve(
    pinned: Option<&OsStr>,
    path: &OsStr,
    retry: bool,
) -> io::Result<(PathBuf, Option<String>)> {
    let pin = pinned.map(PathBuf::from);
    if !retry && let Some(pin) = pin.as_ref().filter(|p| executable(p)) {
        return Ok((std::path::absolute(pin)?, None));
    }
    let fallback = std::env::split_paths(path)
        .map(|p| p.join("tmux"))
        .filter(|p| !retry || pin.as_ref().is_none_or(|pin| p != pin))
        .find(|p| executable(p));
    if let Some(fallback) = fallback {
        let warning = pin.map(|pin| {
            format!(
                "pinned tmux {} unavailable or failed; falling back to PATH ({})",
                pin.display(),
                fallback.display()
            )
        });
        return Ok((std::path::absolute(fallback)?, warning));
    }
    if let Some(pin) = pin.filter(|p| executable(p)) {
        return Ok((std::path::absolute(pin)?, None));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("tmux not found (looked in PATH={})", path.to_string_lossy()),
    ))
}
fn current() -> io::Result<(PathBuf, Option<String>)> {
    resolve(
        std::env::var_os("TMUX_SIDECAR_TMUX").as_deref(),
        &std::env::var_os("PATH").unwrap_or_default(),
        RETRY_PATH.load(Ordering::Relaxed),
    )
}
pub fn resolve_binary() -> io::Result<PathBuf> {
    let (path, warning) = current()?;
    if let Some(warning) = warning
        && !WARNED.swap(true, Ordering::Relaxed)
    {
        eprintln!("tmux-sidecar: {warning}");
    }
    Ok(path)
}
pub(crate) fn for_spawn(paths: &crate::paths::Paths) -> io::Result<PathBuf> {
    let (path, warning) = current()?;
    if let Some(warning) = warning
        && !WARNED.swap(true, Ordering::Relaxed)
    {
        let _ = crate::daemon::log::write(&paths.state, &warning);
    }
    Ok(path)
}
pub(crate) fn retry_path() {
    RETRY_PATH.store(true, Ordering::Relaxed);
}
pub(super) fn quiet_binary() -> PathBuf {
    current()
        .map(|(path, _)| path)
        .unwrap_or_else(|_| PathBuf::from("tmux"))
}
pub(super) fn binary() -> PathBuf {
    resolve_binary().unwrap_or_else(|_| PathBuf::from("tmux"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_non_executable_pins_use_path() {
        let root = tempfile::tempdir().unwrap();
        let fallback = root.path().join("tmux");
        std::fs::write(&fallback, b"fixture").unwrap();
        std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o700)).unwrap();
        let pin = root.path().join("pinned");
        for present in [false, true] {
            if present {
                std::fs::write(&pin, b"not executable").unwrap();
                std::fs::set_permissions(&pin, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            let (chosen, warning) =
                resolve(Some(pin.as_os_str()), root.path().as_os_str(), false).unwrap();
            assert_eq!(chosen, fallback);
            assert!(warning.unwrap().contains("falling back to PATH"));
        }
        std::fs::set_permissions(&pin, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            resolve(Some(pin.as_os_str()), root.path().as_os_str(), true)
                .unwrap()
                .0,
            fallback
        );
    }
}
