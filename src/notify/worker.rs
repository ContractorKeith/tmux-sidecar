//! One notification consumer; backend execution is replaceable by a recording fake.
use super::{Config, Debounce, command};
use std::{future::Future, io, path::PathBuf, time::Instant};
use tokio::sync::broadcast;
pub(super) trait Backend {
    fn desktop(&mut self, message: &str) -> impl Future<Output = io::Result<()>> + Send;
    fn waiting(
        &mut self,
        pane: &str,
        start: u64,
        message: &str,
    ) -> impl Future<Output = io::Result<()>> + Send;
}
pub(super) struct Real {
    pub socket: PathBuf,
}
impl Backend for Real {
    async fn desktop(&mut self, message: &str) -> io::Result<()> {
        command::desktop(message).await
    }
    async fn waiting(&mut self, pane: &str, start: u64, message: &str) -> io::Result<()> {
        crate::tmux::waiting(&self.socket, pane, start, message).await
    }
}
pub(super) async fn run(
    mut backend: impl Backend,
    state: PathBuf,
    config: Config,
    mut events: broadcast::Receiver<serde_json::Value>,
) {
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
        if !debounce.allow(&config, id, to, Instant::now()) {
            continue;
        }
        let message = format!("Agent run {id} is {to}");
        let result = if to == "waiting" {
            let Some(pane) = event["pane_id"].as_str() else {
                continue;
            };
            let Some(server_start) = event["server_start"].as_u64() else {
                continue;
            };
            backend.waiting(pane, server_start, &message).await
        } else {
            backend.desktop(&message).await
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
                crate::daemon::log::write(&log_state, &format!("{backend} unavailable: {error}"))
            })
            .await;
        }
    }
}
