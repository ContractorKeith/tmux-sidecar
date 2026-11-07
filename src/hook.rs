//! Hook input is drained and a whole durable frame is appended before live delivery.
pub(crate) mod attribution;
pub mod context;
mod delivery;
pub(crate) mod files;
pub mod input;
pub mod signal;
pub mod spool;
use std::io;
pub async fn entry(args: Vec<String>) {
    let _ = delivery::run(args).await;
}
pub fn log(state: &std::path::Path, error: &dyn std::fmt::Display) -> io::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };
    let path = state.join("hook.log");
    let mut file = files::regular(OpenOptions::new().create(true).append(true), &path)?;
    if file.metadata()?.len() >= 1024 * 1024 {
        fs::rename(&path, state.join("hook.log.1"))?;
        file = files::regular(OpenOptions::new().create(true).append(true), &path)?;
    }
    writeln!(file, "{error}")
}

/// Invalid formats still have a log destination when invoked outside tmux.
pub(crate) fn unknown_format() -> io::Result<()> {
    let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME missing"))?;
    let platform = if cfg!(target_os = "macos") {
        crate::paths::Platform::Macos
    } else {
        crate::paths::Platform::Linux
    };
    let scoped = std::env::var_os("TMUX")
        .and_then(|value| crate::paths::socket_from_tmux(&value))
        .and_then(|socket| crate::paths::current(&socket).ok());
    let paths = scoped.unwrap_or_else(|| {
        crate::paths::resolve(
            platform,
            std::path::Path::new(&home),
            crate::paths::uid(),
            "unscoped",
            |name| std::env::var_os(name),
        )
    });
    crate::paths::prepare(&paths)?;
    log(&paths.state, &"unknown hook format")
}
