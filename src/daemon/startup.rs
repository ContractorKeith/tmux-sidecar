use super::{log, recovery, store_worker::StoreWorker};
use crate::paths::Paths;
use std::{fs, io, os::unix::fs::PermissionsExt};
use tokio::net::UnixListener;
pub(super) async fn store(paths: &Paths) -> io::Result<StoreWorker> {
    let db = paths.state.join("tmux-sidecar.db");
    let recreated = recovery::was_deleted(&paths.state, &db)?;
    if !db.exists() && !recreated {
        log::write(
            &paths.state,
            &format!("creating fresh database {}", db.display()),
        )?;
    }
    let store = StoreWorker::open(db).await?;
    recovery::created(paths, recreated)?;
    Ok(store)
}
pub(super) fn listener(paths: &Paths) -> io::Result<UnixListener> {
    let listener = UnixListener::bind(paths.runtime.join("sock"))?;
    fs::set_permissions(
        paths.runtime.join("sock"),
        fs::Permissions::from_mode(0o600),
    )?;
    fs::write(
        paths.runtime.join("daemon.pid"),
        std::process::id().to_string(),
    )?;
    fs::write(
        paths.state.join("daemon.socket.next"),
        std::path::absolute(paths.runtime.join("sock"))?
            .as_os_str()
            .as_encoded_bytes(),
    )?;
    fs::rename(
        paths.state.join("daemon.socket.next"),
        paths.state.join("daemon.socket"),
    )?;
    Ok(listener)
}

pub async fn run(socket: std::path::PathBuf, paths: Paths) -> io::Result<()> {
    let result = super::run_inner(socket, &paths).await;
    if let Err(error) = &result
        && crate::paths::prepare(&paths).is_ok()
    {
        let _ = log::write(&paths.state, &error.to_string());
    }
    result
}
