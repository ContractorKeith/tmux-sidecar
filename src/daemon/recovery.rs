//! A startup marker distinguishes a missing history DB from a first installation.
use crate::{hook::files, paths::Paths};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};
pub(crate) const NOTICE: &str = "history database was deleted; a new one was created";

pub(super) fn was_deleted(state: &Path, database: &Path) -> io::Result<bool> {
    if database.try_exists()? {
        return Ok(false);
    }
    match files::read(&state.join("history.created")) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
fn write(path: &Path, text: &str) -> io::Result<()> {
    files::regular(
        OpenOptions::new().create(true).write(true).truncate(true),
        path,
    )?
    .write_all(text.as_bytes())
}
pub(super) fn created(paths: &Paths, recreated: bool) -> io::Result<()> {
    write(&paths.state.join("history.created"), "1\n")?;
    if recreated {
        super::log::write(&paths.state, NOTICE)?;
        write(&paths.runtime.join("history.notice"), NOTICE)?;
        if std::env::var_os("TMUX_SIDECAR_DETACHED").is_none() {
            eprintln!("tmux-sidecar: {NOTICE}");
        }
    }
    Ok(())
}
// Called after hello validates the server. Hooks use verify(), never this output path.
pub(crate) fn report(paths: &Paths) {
    let path = paths.runtime.join("history.notice");
    if files::read(&path).is_ok_and(|bytes| bytes == NOTICE.as_bytes())
        && fs::remove_file(path).is_ok()
    {
        eprintln!("tmux-sidecar: {NOTICE}");
    }
}
