//! Desktop notifications consume committed changes outside the store and RPC tasks.
mod command;
#[cfg(test)]
mod tests;
mod worker;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub desktop: bool,
    pub tmux: bool,
    pub debounce_secs: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            desktop: true,
            tmux: true,
            debounce_secs: 10,
        }
    }
}
impl Config {
    pub fn read(path: &Path) -> io::Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let table: toml::Table = toml::from_str(&text).map_err(io::Error::other)?;
        table
            .get("notify")
            .cloned()
            .map(|v| v.try_into().map_err(io::Error::other))
            .unwrap_or_else(|| Ok(Self::default()))
    }
}
#[derive(Default)]
struct Debounce {
    runs: HashMap<String, Instant>,
    global: Option<Instant>,
}
impl Debounce {
    fn allow(&mut self, config: &Config, id: &str, state: &str, now: Instant) -> bool {
        if !match state {
            "waiting" => config.tmux,
            "blocked" | "error" => config.desktop,
            _ => false,
        } {
            return false;
        }
        self.runs.retain(|_, at| {
            now.saturating_duration_since(*at) < Duration::from_secs(config.debounce_secs)
        });
        if self
            .global
            .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(2))
            || (state != "waiting" && self.runs.contains_key(id))
        {
            return false;
        }
        if state != "waiting" {
            self.runs.insert(id.into(), now);
        }
        self.global = Some(now);
        true
    }
}
pub(crate) fn start(
    socket: PathBuf,
    state: PathBuf,
    config: Config,
    events: broadcast::Receiver<serde_json::Value>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(worker::run(worker::Real { socket }, state, config, events))
}
