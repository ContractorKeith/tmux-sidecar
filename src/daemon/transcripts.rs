//! Source reads run outside both the RPC tasks and the SQLite owner.
use super::store_worker::StoreWorker;
use crate::model::{AgentKind, AgentRun};
use session_board::{
    config::Config,
    model::{Agent, Session},
    source,
};
use std::{collections::HashMap, io, path::Path};
#[derive(Default, Clone)]
pub struct Reader {
    last: std::sync::Arc<std::sync::Mutex<HashMap<String, u64>>>,
}
impl Reader {
    pub async fn tick(&self, worker: &StoreWorker, now: u64) -> io::Result<()> {
        let runs = worker.with(|s| s.active()).await??;
        let due = {
            let mut last = self
                .last
                .lock()
                .map_err(|_| io::Error::other("transcript schedule poisoned"))?;
            last.retain(|id, _| runs.iter().any(|r| &r.id == id));
            let due: Vec<_> = runs
                .into_iter()
                .filter(|r| {
                    last.get(&r.id)
                        .is_none_or(|at| now.saturating_sub(*at) >= 2000)
                })
                .collect();
            for run in &due {
                last.insert(run.id.clone(), now);
            }
            due
        };
        if due.is_empty() {
            return Ok(());
        }
        let readings = tokio::task::spawn_blocking(move || read(&due))
            .await
            .map_err(io::Error::other)?;
        worker
            .with(move |store| {
                for (id, reading) in readings {
                    store.transcript(&id, &reading, now)?;
                }
                Ok::<_, io::Error>(())
            })
            .await?
    }
}
fn read(runs: &[AgentRun]) -> Vec<(String, Session)> {
    let config = Config {
        window_days: u32::MAX,
        ..Config::default()
    };
    let mut sessions = Vec::new();
    for source in source::registry() {
        if runs.iter().any(|r| agent(r.agent) == source.agent()) {
            sessions.extend(source.scan(&config).sessions);
        }
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity_at));
    let mut used = std::collections::HashSet::new();
    runs.iter()
        .filter_map(|run| {
            let cwd = Path::new(&run.cwd).canonicalize().ok()?;
            let reading = sessions.iter().find(|s| {
                agent(run.agent) == s.agent
                    && s.cwd.canonicalize().is_ok_and(|p| p == cwd)
                    && run.agent_session_id.as_ref().is_none_or(|id| {
                        id == &s.id || s.id.split_once(':').is_some_and(|(_, native)| native == id)
                    })
                    && !used.contains(&s.transcript_path)
            })?;
            used.insert(reading.transcript_path.clone());
            Some((run.id.clone(), reading.clone()))
        })
        .collect()
}
fn agent(kind: AgentKind) -> Agent {
    match kind {
        AgentKind::Claude => Agent::ClaudeCode,
        AgentKind::Codex => Agent::Codex,
        AgentKind::Grok => Agent::Grok,
        AgentKind::Opencode => Agent::Opencode,
    }
}
