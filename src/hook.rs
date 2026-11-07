//! Drain hook input; attempt durable append before live delivery, even when append fails.
pub(crate) mod attribution;
pub mod context;
mod delivery;
pub(crate) mod files;
pub mod input;
pub mod signal;
pub mod spool;
use std::io;
tokio::task_local! { static LOG_FORMAT: String; }
pub async fn entry(args: Vec<String>) {
    let format = args.first().cloned().unwrap_or_else(|| "unknown".into());
    let _ = LOG_FORMAT.scope(format, delivery::run(args)).await;
}
pub fn log(state: &std::path::Path, error: &dyn std::fmt::Display) -> io::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
    };
    let format = LOG_FORMAT
        .try_with(Clone::clone)
        .unwrap_or_else(|_| "unknown".into());
    let pane = std::env::var("TMUX_PANE").unwrap_or_else(|_| "unknown".into());
    let agent = log_agent(&pane);
    let mut line = format!(
        "{} agent={} format={} pane={} {}",
        crate::daemon::log::timestamp(),
        agent,
        format.escape_default(),
        pane.escape_default(),
        error.to_string().replace(['\n', '\r'], " ")
    );
    line.truncate(line.floor_char_boundary(1024 * 1024 - 1));
    line.push('\n');
    let path = state.join("hook.log");
    let mut file = files::regular(OpenOptions::new().create(true).append(true), &path)?;
    if file.metadata()?.len().saturating_add(line.len() as u64) >= 1024 * 1024 {
        fs::rename(&path, state.join("hook.log.1"))?;
        file = files::regular(OpenOptions::new().create(true).append(true), &path)?;
    }
    file.write_all(line.as_bytes())
}

// Use the cached pane boundary when available; logging never runs a tmux probe.
fn log_agent(pane: &str) -> String {
    let chain = crate::proc::hook_ancestors().unwrap_or_default();
    let bounded = (|| {
        let tmux = std::env::var_os("TMUX")?;
        let socket = crate::paths::socket_from_tmux(&tmux)?;
        let paths = crate::paths::current(&socket).ok()?;
        let start = context::read(&paths.runtime, &socket).ok()?;
        let cache: serde_json::Value =
            serde_json::from_slice(&files::read(&paths.runtime.join("pane_pids")).ok()?).ok()?;
        if cache["server_start"] != start {
            return None;
        }
        let pid = u32::try_from(cache["panes"][pane].as_u64()?).ok()?;
        attribution::ancestor(&chain, pid)
    })();
    bounded
        .or_else(|| {
            chain
                .iter()
                .find_map(|(pid, p)| crate::proc::agent_kind(&p.comm).map(|agent| (*pid, agent)))
        })
        .map(|(_, agent)| format!("{agent:?}").to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".into())
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
