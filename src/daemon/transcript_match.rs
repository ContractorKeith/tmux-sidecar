use crate::model::{AgentKind, AgentRun};
use session_board::model::{Agent, Session};
use std::{collections::HashSet, path::Path};
pub fn agent(kind: AgentKind) -> Agent {
    match kind {
        AgentKind::Claude => Agent::ClaudeCode,
        AgentKind::Codex => Agent::Codex,
        AgentKind::Grok => Agent::Grok,
        AgentKind::Opencode => Agent::Opencode,
    }
}
pub fn assign(runs: &[AgentRun], sessions: &[Session]) -> Vec<(String, Session)> {
    let mut used = HashSet::new();
    let mut assigned = Vec::new();
    // Reserve explicit identities, including hooked runs, before cwd-only fallback.
    for has_id in [true, false] {
        for run in runs
            .iter()
            .filter(|r| r.agent_session_id.is_some() == has_id)
        {
            let Ok(cwd) = Path::new(&run.cwd).canonicalize() else {
                continue;
            };
            let reading = sessions.iter().find(|s| {
                agent(run.agent) == s.agent
                    && s.cwd.canonicalize().is_ok_and(|p| p == cwd)
                    && run.agent_session_id.as_ref().is_none_or(|id| {
                        id == &s.id || s.id.split_once(':').is_some_and(|(_, native)| native == id)
                    })
                    && !used.contains(&s.transcript_path)
            });
            if let Some(reading) = reading {
                used.insert(reading.transcript_path.clone());
                assigned.push((run.id.clone(), reading.clone()));
            }
        }
    }
    assigned
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn explicit_session_reserves_transcript_before_older_unbound_run() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path();
        let run:AgentRun=serde_json::from_value(json!({"id":"r1","server_start":1,"pane_id":"%0","agent":"claude","agent_pid":1,"agent_session_id":null,"cwd":cwd,"project_root":null,"started_at":1000,"ended_at":null,"state":"working","state_since":1000,"state_source":"process","read_at":null,"last_assistant_text":"","transcript_path":null})).unwrap();
        let mut bound = run.clone();
        bound.id = "r2".into();
        bound.agent_session_id = Some("known".into());
        let session:Session=serde_json::from_value(json!({"id":"claude_code:known","agent":"claude_code","cwd":cwd,"project_root":cwd,"title":"known","started_at":1,"last_activity_at":3,"status":"working","last_user_text":"","last_assistant_text":"","transcript_path":cwd.join("known.jsonl"),"resume_hint":"","pane":null})).unwrap();
        let mut other = session.clone();
        other.id = "claude_code:other".into();
        other.transcript_path = cwd.join("other.jsonl");
        let result = assign(&[run, bound], &[session, other]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "r2");
        assert_eq!(result[0].1.id, "claude_code:known");
        assert_eq!(result[1].0, "r1");
        assert_eq!(result[1].1.id, "claude_code:other");
    }
}
