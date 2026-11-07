//! One notification consumer; backend execution is replaceable by a recording fake.
use super::{Config, Debounce, command};
use std::{future::Future, io, path::PathBuf, time::Instant};
use tokio::sync::broadcast;
pub(super) trait Backend {
    fn project_root(&mut self, _id: &str) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
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
    async fn project_root(&mut self, id: &str) -> Option<String> {
        let paths = crate::paths::current(&self.socket).ok()?;
        let runs = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            crate::client::request(&self.socket, &paths, "list", serde_json::json!({}), false),
        )
        .await
        .ok()?
        .ok()?;
        runs.as_array()?.iter().find(|run| run["id"] == id)?["project_root"]
            .as_str()
            .map(str::to_owned)
    }
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
            let root = match event["project_root"].as_str() {
                Some(root) => Some(root.to_owned()),
                None => backend.project_root(id).await,
            };
            let project = root
                .as_deref()
                .and_then(|root| std::path::Path::new(root).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("unknown project");
            let agent = event["agent"].as_str().unwrap_or("agent");
            let pane = event["pane_name"]
                .as_str()
                .or_else(|| event["pane_id"].as_str())
                .unwrap_or("unknown pane");
            backend
                .desktop(&format!("{agent} in {project} ({pane}) is {to}"))
                .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Recording(Arc<Mutex<Vec<String>>>);
    impl Backend for Recording {
        async fn desktop(&mut self, message: &str) -> io::Result<()> {
            self.0.lock().unwrap().push(message.into());
            Ok(())
        }
        async fn waiting(&mut self, _: &str, _: u64, _: &str) -> io::Result<()> {
            panic!("desktop transition must not send a tmux message")
        }
    }
    #[tokio::test]
    async fn desktop_uses_project_basename_and_name_or_pane_id() {
        for (name, pane) in [(Some("review"), "review"), (None, "%7")] {
            let root = tempfile::tempdir().unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let (send, receive) = broadcast::channel(1);
            send.send(serde_json::json!({"run_id":"r1","from":"working","to":"blocked", "agent":"grok", "project_root":"/work/project", "pane_name":name, "pane_id":"%7"})).unwrap();
            drop(send);
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                run(
                    Recording(calls.clone()),
                    root.path().into(),
                    Config::default(),
                    receive,
                ),
            )
            .await
            .unwrap();
            assert_eq!(
                *calls.lock().unwrap(),
                [format!("grok in project ({pane}) is blocked")]
            );
        }
    }
}
