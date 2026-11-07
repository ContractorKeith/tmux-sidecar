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
        let deadline = debounce.deadline();
        let closed = tokio::select! {
            event = events.recv() => match event {
                Ok(event) => { debounce.push(&config, event, Instant::now()); false }
                Err(broadcast::error::RecvError::Lagged(_)) => false,
                Err(_) => true,
            },
            _ = async {
                match deadline {
                    Some(at) => tokio::time::sleep_until(at.into()).await,
                    None => std::future::pending::<()>().await,
                }
            } => false,
        };
        let batches = if closed {
            debounce.finish()
        } else {
            debounce.drain(Instant::now())
        };
        for batch in batches {
            let waiting = batch[0]["to"] == "waiting";
            let result = deliver(&mut backend, &batch, waiting).await;
            let kind = usize::from(waiting);
            if let Err(error) = result
                && !warned[kind]
            {
                warned[kind] = true;
                let backend = if waiting {
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
        if closed {
            break;
        }
    }
}
async fn deliver(
    backend: &mut impl Backend,
    batch: &[serde_json::Value],
    waiting: bool,
) -> io::Result<()> {
    let mut descriptions = Vec::new();
    for event in batch {
        let root = match event["project_root"].as_str() {
            Some(root) => Some(root.to_owned()),
            None => {
                backend
                    .project_root(
                        event["run_id"]
                            .as_str()
                            .expect("queued event has validated text"),
                    )
                    .await
            }
        };
        let project = root
            .as_deref()
            .and_then(|r| std::path::Path::new(r).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown project");
        let agent = event["agent"].as_str().unwrap_or("agent");
        let pane = event["pane_name"]
            .as_str()
            .or_else(|| event["pane_id"].as_str())
            .unwrap_or("unknown pane");
        descriptions.push(format!("{agent} in {project} ({pane})"));
    }
    let to = batch[0]["to"]
        .as_str()
        .expect("queued event has validated text");
    let message = if batch.len() == 1 {
        if waiting {
            format!(
                "Agent run {} is waiting",
                batch[0]["run_id"]
                    .as_str()
                    .expect("queued event has validated text")
            )
        } else {
            format!("{} is {to}", descriptions[0])
        }
    } else if batch.iter().all(|e| e["to"] == to) {
        format!("{} runs {to}: {}", batch.len(), descriptions.join(", "))
    } else {
        let details: Vec<_> = descriptions
            .iter()
            .zip(batch)
            .map(|(d, e)| {
                format!(
                    "{d} is {}",
                    e["to"].as_str().expect("queued event has validated text")
                )
            })
            .collect();
        format!(
            "{} runs need attention: {}",
            batch.len(),
            details.join(", ")
        )
    };
    if waiting {
        // Deliver the summary to every affected tmux session, not just its first pane.
        for event in batch {
            backend
                .waiting(
                    event["pane_id"]
                        .as_str()
                        .expect("queued event has validated text"),
                    event["server_start"]
                        .as_u64()
                        .expect("queued waiting event has server_start"),
                    &message,
                )
                .await?;
        }
        Ok(())
    } else {
        backend.desktop(&message).await
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
