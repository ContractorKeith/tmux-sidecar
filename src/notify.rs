//! Desktop notifications consume committed changes outside the store and RPC tasks.
mod command;
#[cfg(test)]
mod tests;
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
    pub debounce_secs: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            desktop: true,
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
        if !config.desktop || !matches!(state, "blocked" | "error") {
            return false;
        }
        self.runs.retain(|_, at| {
            now.saturating_duration_since(*at) < Duration::from_secs(config.debounce_secs)
        });
        if self
            .global
            .is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(2))
            || self.runs.contains_key(id)
        {
            return false;
        }
        self.runs.insert(id.into(), now);
        self.global = Some(now);
        true
    }
}
pub(crate) fn start(
    socket: PathBuf,
    state: PathBuf,
    config: Config,
    mut events: broadcast::Receiver<serde_json::Value>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut debounce = Debounce::default();
        let mut warned = [false; 2];
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            let Some(id) = event["run_id"].as_str() else {
                continue;
            };
            let Some(to) = event["to"].as_str() else {
                continue;
            };
            if event["from"] == event["to"] {
                continue;
            }
            let message = format!("Agent run {id} is {to}");
            let result = if to == "waiting" {
                // Resolve pane data through the socket, never by sharing the SQLite owner.
                match crate::client::request(
                    &socket,
                    &match crate::paths::current(&socket) {
                        Ok(p) => p,
                        Err(_) => continue,
                    },
                    "list",
                    serde_json::json!({}),
                    false,
                )
                .await
                {
                    Ok(runs) => {
                        if let Some(run) = runs
                            .as_array()
                            .and_then(|runs| runs.iter().find(|run| run["id"] == id))
                        {
                            match run["pane_id"].as_str() {
                                Some(pane) => crate::tmux::waiting(&socket, pane, &message).await,
                                None => Ok(()),
                            }
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => Err(e),
                }
            } else if debounce.allow(&config, id, to, Instant::now()) {
                command::desktop(&message).await
            } else {
                continue;
            };
            let warning_kind = usize::from(to == "waiting");
            if let Err(error) = result
                && !warned[warning_kind]
            {
                warned[warning_kind] = true;
                let backend = if to == "waiting" {
                    "tmux waiting message"
                } else {
                    "desktop notifier"
                };
                let log_state = state.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    crate::daemon::log::write(
                        &log_state,
                        &format!("{backend} unavailable: {error}"),
                    )
                })
                .await;
            }
        }
    })
}
