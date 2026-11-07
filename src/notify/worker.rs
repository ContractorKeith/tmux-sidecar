//! One notification consumer; backend execution is replaceable by a recording fake.
pub(super) use super::backend::Backend;
use super::{Config, Debounce};
use std::{io, path::PathBuf, time::Instant};
use tokio::sync::broadcast;
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
    let mut current = Vec::new();
    for event in batch {
        if waiting || backend.current(event).await {
            current.push(event.clone());
        }
    }
    let batch = current.as_slice();
    if batch.is_empty() {
        return Ok(());
    }
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
        let mut failure = None;
        for event in batch {
            if let Err(error) = backend
                .waiting(
                    event["pane_id"]
                        .as_str()
                        .expect("queued event has validated text"),
                    event["server_start"]
                        .as_u64()
                        .expect("queued waiting event has server_start"),
                    &message,
                )
                .await
            {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    } else {
        backend.desktop(&message).await
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
