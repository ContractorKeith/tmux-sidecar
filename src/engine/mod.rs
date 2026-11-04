//! Pure process-based run detection. M3 adds signal-driven states.
use crate::{
    model::{AgentKind, AgentRun, AgentState},
    seams::{PaneSnapshot, ProcessSnapshot},
};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Changes {
    pub start: Vec<AgentRun>,
    pub end: Vec<String>,
}

pub fn detect(
    panes: &[PaneSnapshot],
    processes: &ProcessSnapshot,
    runs: &[AgentRun],
    now: u64,
) -> Changes {
    detect_scan(panes, &[], processes, runs, now)
}
pub fn detect_scan(
    panes: &[PaneSnapshot],
    skipped: &[String],
    processes: &ProcessSnapshot,
    runs: &[AgentRun],
    now: u64,
) -> Changes {
    let mut detected = Vec::new();
    for pane in panes {
        // A pane may execute the agent directly, so include pane_pid itself.
        let mut candidates = Vec::new();
        for (&pid, process) in processes {
            let Some(agent) = kind(&process.comm) else {
                continue;
            };
            let mut cursor = pid;
            let mut seen = HashSet::new();
            while seen.insert(cursor) {
                if cursor != pid
                    && processes
                        .get(&cursor)
                        .is_some_and(|p| kind(&p.comm).is_some())
                {
                    break;
                }
                if cursor == pane.pane_pid {
                    candidates.push((pid, agent));
                    break;
                }
                let Some(parent) = processes.get(&cursor) else {
                    break;
                };
                cursor = parent.ppid;
            }
        }
        // Ignore nested agent helpers; newest independent agent pid wins.
        candidates.sort_by_key(|&(pid, _)| std::cmp::Reverse(pid));
        if let Some(&(pid, agent)) = candidates.first() {
            detected.push(AgentRun {
                id: String::new(),
                server_start: pane.server_start,
                pane_id: pane.pane_id.clone(),
                agent,
                agent_pid: pid,
                agent_session_id: None,
                cwd: pane.cwd.to_string_lossy().into(),
                project_root: None,
                started_at: now,
                ended_at: None,
                state: AgentState::Waiting,
                state_since: now,
                state_source: "process".into(),
                read_at: None,
                last_assistant_text: String::new(),
                transcript_path: None,
            });
        }
    }
    let same = |a: &AgentRun, b: &AgentRun| {
        a.server_start == b.server_start && a.pane_id == b.pane_id && a.agent_pid == b.agent_pid
    };
    Changes {
        end: runs
            .iter()
            .filter(|r| {
                r.ended_at.is_none()
                    && !skipped.contains(&r.pane_id)
                    && !detected.iter().any(|d| same(r, d))
            })
            .map(|r| r.id.clone())
            .collect(),
        start: detected
            .into_iter()
            .filter(|d| !runs.iter().any(|r| r.ended_at.is_none() && same(r, d)))
            .collect(),
    }
}

fn kind(comm: &str) -> Option<AgentKind> {
    match comm {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "grok" => Some(AgentKind::Grok),
        "opencode" => Some(AgentKind::Opencode),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
