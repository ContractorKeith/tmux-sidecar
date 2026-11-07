use super::*;
use crate::hook::signal::Signal;
impl Store {
    pub(crate) fn orphan_owner(&self, signal: &Signal) -> io::Result<Option<AgentRun>> {
        let runs: Vec<_> = self
            .active()?
            .into_iter()
            .filter(|r| r.server_start == signal.server_start && r.agent == signal.agent)
            .collect();
        let unique = |candidates: Vec<AgentRun>| {
            if candidates.len() == 1 {
                candidates.into_iter().next()
            } else {
                None
            }
        };
        if let Some(id) = &signal.agent_session_id {
            let matching: Vec<_> = runs
                .iter()
                .filter(|r| r.agent_session_id.as_ref() == Some(id))
                .cloned()
                .collect();
            if !matching.is_empty() {
                return Ok(unique(matching));
            }
        }
        if let Some(pid) = signal.parent_agent_pid {
            let matching: Vec<_> = runs
                .iter()
                .filter(|r| r.agent_pid == pid)
                .cloned()
                .collect();
            if !matching.is_empty() {
                return Ok(unique(matching));
            }
        }
        Ok(unique(
            runs.into_iter()
                .filter(|r| {
                    r.agent_session_id.is_none() && !signal.cwd.is_empty() && r.cwd == signal.cwd
                })
                .collect(),
        ))
    }
}
