//! Desktop notifications consume committed changes outside the store and RPC tasks.
mod backend;
#[cfg(test)]
mod burst_tests;
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
            Err(e) => return Err(config_error(path, e)),
        };
        let table: toml::Table = toml::from_str(&text).map_err(|e| config_error(path, e))?;
        table
            .get("notify")
            .cloned()
            .map(|v| v.try_into().map_err(|e| config_error(path, e)))
            .unwrap_or_else(|| Ok(Self::default()))
    }
}
fn config_error(path: &Path, error: impl std::fmt::Display) -> io::Error {
    let reason = error
        .to_string()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    io::Error::other(format!("config {}: {reason}", path.display()))
}
#[derive(Default)]
struct Window {
    next: Option<Instant>,
    pending: Vec<serde_json::Value>,
}
#[derive(Default)]
struct Debounce {
    runs: HashMap<(String, String), (serde_json::Value, Instant)>,
    windows: [Window; 2],
}
impl Debounce {
    fn push(&mut self, config: &Config, event: serde_json::Value, now: Instant) {
        let (Some(id), Some(to)) = (event["run_id"].as_str(), event["to"].as_str()) else {
            return;
        };
        if event["from"] == event["to"] {
            return;
        }
        if to == "done" {
            self.runs.retain(|(run, _), _| run != id);
        }
        let channel = match to {
            "waiting"
                if config.tmux
                    && event["pane_id"].is_string()
                    && event["server_start"].is_u64() =>
            {
                1
            }
            "blocked" | "error" if config.desktop => 0,
            _ => return,
        };
        let key = (id.to_owned(), to.to_owned());
        if let Some((episode, at)) = self.runs.get(&key) {
            let duplicate = if channel == 0 {
                event["at"] == *episode
            } else {
                now.saturating_duration_since(*at) < Duration::from_secs(config.debounce_secs)
            };
            if duplicate {
                return;
            }
        }
        self.runs.insert(key, (event["at"].clone(), now));
        let window = &mut self.windows[channel];
        if window.pending.is_empty() {
            window.next = Some(now + Duration::from_secs(2));
        }
        window.pending.push(event);
    }
    fn deadline(&self) -> Option<Instant> {
        self.windows
            .iter()
            .filter(|w| !w.pending.is_empty())
            .map(|w| w.next.unwrap_or_else(Instant::now))
            .min()
    }
    fn drain(&mut self, now: Instant) -> Vec<Vec<serde_json::Value>> {
        let mut batches = Vec::new();
        for window in &mut self.windows {
            if !window.pending.is_empty() && window.next.is_none_or(|at| now >= at) {
                batches.push(std::mem::take(&mut window.pending));
                window.next = None;
            }
        }
        batches
    }
    fn finish(&mut self) -> Vec<Vec<serde_json::Value>> {
        self.windows
            .iter_mut()
            .filter(|w| !w.pending.is_empty())
            .map(|w| std::mem::take(&mut w.pending))
            .collect()
    }
}
pub(crate) fn start(
    socket: PathBuf,
    state: PathBuf,
    config: Config,
    events: broadcast::Receiver<serde_json::Value>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(worker::run(backend::Real { socket }, state, config, events))
}
