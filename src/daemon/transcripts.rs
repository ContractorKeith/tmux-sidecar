use super::{store_worker::StoreWorker, transcript_match};
use crate::model::AgentRun;
use session_board::{config::Config, model::Session, source};
use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
#[derive(Default)]
pub struct Reader {
    running: Option<tokio::task::JoinHandle<()>>,
}
impl Reader {
    pub fn start(&mut self, worker: StoreWorker, state: PathBuf) {
        self.start_with(worker, state, read);
    }
    fn start_with(
        &mut self,
        worker: StoreWorker,
        state: PathBuf,
        scan: impl FnOnce(&[AgentRun], u64) -> Vec<Session> + Send + 'static,
    ) {
        if self
            .running
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        self.running = Some(tokio::spawn(async move {
            if let Err(error) = tick(&worker, scan).await {
                let _ = tokio::task::spawn_blocking(move || {
                    super::log::write(&state, &format!("transcript scan: {error}"))
                })
                .await;
            }
        }));
    }
    pub async fn stop(&mut self) {
        if let Some(task) = self.running.take() {
            task.abort();
            let _ = task.await;
        }
    }
}
async fn tick(
    worker: &StoreWorker,
    scan: impl FnOnce(&[AgentRun], u64) -> Vec<Session> + Send + 'static,
) -> io::Result<()> {
    let (runs, eligible) = worker
        .with(|store| {
            let runs = store.active()?;
            let eligible = runs
                .iter()
                .filter_map(|r| match store.has_hooks(&r.id) {
                    Ok(false) => Some(Ok(r.clone())),
                    Ok(true)
                        if r.agent == crate::model::AgentKind::Claude
                            && r.state == crate::model::AgentState::Blocked =>
                    {
                        Some(Ok(r.clone()))
                    }
                    Ok(true) => None,
                    Err(e) => Some(Err(e)),
                })
                .collect::<io::Result<Vec<_>>>()?;
            Ok::<_, io::Error>((runs, eligible))
        })
        .await??;
    let Some(since) = eligible.iter().map(|r| r.started_at).min() else {
        return Ok(());
    };
    let readings = tokio::task::spawn_blocking(move || {
        let sessions = scan(&eligible, since);
        transcript_match::assign(&runs, &sessions)
            .into_iter()
            .filter(|(id, _)| eligible.iter().any(|r| &r.id == id))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(io::Error::other)?;
    worker
        .with(move |store| {
            for (id, reading) in readings {
                store.transcript(&id, &reading, super::now())?;
            }
            Ok::<_, io::Error>(())
        })
        .await?
}
fn read(runs: &[AgentRun], since: u64) -> Vec<Session> {
    #[cfg(feature = "test-util")]
    if let Some(gate) = std::env::var_os("TMUX_SIDECAR_TEST_SCAN_GATE") {
        let gate = PathBuf::from(gate);
        let _ = std::fs::write(gate.join("scan-started"), b"");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !gate.join("scan-release").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let age = super::now().saturating_sub(since);
    let config = Config {
        window_days: u32::try_from(age / 86_400_000 + 1).unwrap_or(u32::MAX),
        ..Config::default()
    };
    let cutoff =
        SystemTime::now().checked_sub(Duration::from_secs(u64::from(config.window_days) * 86_400));
    let include = |path: &Path| recent(path, cutoff);
    let mut sessions = Vec::new();
    for source in source::registry() {
        if runs
            .iter()
            .any(|r| transcript_match::agent(r.agent) == source.agent())
        {
            sessions.extend(source.scan_files(&config, &include).sessions);
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity_at));
    sessions
}
fn recent(path: &Path, cutoff: Option<SystemTime>) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map_or(true, |modified| modified >= cutoff)
}
#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
