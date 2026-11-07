use std::os::unix::fs::PermissionsExt;
use std::{io, path::PathBuf};
pub fn resolve_binary() -> io::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let candidates: Vec<_> = match std::env::var_os("TMUX_SIDECAR_TMUX") {
        Some(value) => vec![PathBuf::from(value)],
        None => std::env::split_paths(&path)
            .map(|dir| dir.join("tmux"))
            .collect(),
    };
    for candidate in candidates {
        if candidate
            .metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            return std::path::absolute(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("tmux not found (looked in PATH={})", path.to_string_lossy()),
    ))
}
pub(super) fn binary() -> PathBuf {
    resolve_binary().unwrap_or_else(|_| PathBuf::from("tmux"))
}
