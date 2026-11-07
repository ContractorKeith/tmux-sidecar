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
    // Reserve explicit identities and existing paths before assigning new transcripts.
    let mut ordered: Vec<_> = runs.iter().collect();
    ordered.sort_by_key(|r| {
        (
            r.agent_session_id.is_none(),
            r.transcript_path.as_ref().is_none_or(|path| {
                !sessions
                    .iter()
                    .any(|s| Path::new(path) == s.transcript_path)
            }),
            std::cmp::Reverse(r.started_at),
        )
    });
    for run in ordered {
        let Ok(cwd) = Path::new(&run.cwd).canonicalize() else {
            continue;
        };
        let matches = |s: &&Session| {
            agent(run.agent) == s.agent
                && s.last_activity_at >= 0
                && s.last_activity_at as u64 >= run.started_at / 1000
                && s.cwd.canonicalize().is_ok_and(|p| p == cwd)
                && !used.contains(&s.transcript_path)
                && run.agent_session_id.as_ref().is_none_or(|id| {
                    id == &s.id || s.id.split_once(':').is_some_and(|(_, native)| native == id)
                })
        };
        let preferred = sessions.iter().filter(matches).find(|s| {
            run.transcript_path
                .as_ref()
                .is_some_and(|path| Path::new(path) == s.transcript_path)
        });
        let reading = preferred.or_else(|| {
            sessions.iter().filter(matches).min_by_key(|s| {
                // A resumed transcript can predate the process; prefer the nearest start.
                (
                    s.started_at < (run.started_at / 1000) as i64,
                    s.started_at.abs_diff((run.started_at / 1000) as i64),
                )
            })
        });
        if let Some(reading) = reading {
            used.insert(reading.transcript_path.clone());
            assigned.push((run.id.clone(), reading.clone()));
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
        let mut fresh = run.clone();
        fresh.started_at = 4000;
        assert!(
            assign(std::slice::from_ref(&fresh), std::slice::from_ref(&session)).is_empty(),
            "older reading must not attach to a new run"
        );
        let mut resumed = session.clone();
        resumed.last_activity_at = 4;
        assert_eq!(assign(&[fresh], &[resumed]).len(), 1);
        let result = assign(&[run, bound], &[session, other]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "r2");
        assert_eq!(result[0].1.id, "claude_code:known");
        assert_eq!(result[1].0, "r1");
        assert_eq!(result[1].1.id, "claude_code:other");
    }
    #[test]
    fn new_session_goes_to_closest_unbound_run_and_stays_bound() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path();
        let old:AgentRun=serde_json::from_value(json!({"id":"r1","server_start":1,"pane_id":"%0","agent":"claude","agent_pid":1,"agent_session_id":null,"cwd":cwd,"project_root":null,"started_at":1000,"ended_at":null,"state":"waiting","state_since":1000,"state_source":"transcript","read_at":null,"last_assistant_text":"","transcript_path":cwd.join("old.jsonl")})).unwrap();
        let mut fresh = old.clone();
        fresh.id = "r2".into();
        fresh.started_at = 3000;
        fresh.transcript_path = None;
        let session:Session=serde_json::from_value(json!({"id":"claude_code:new","agent":"claude_code","cwd":cwd,"project_root":cwd,"title":"new","started_at":4,"last_activity_at":5,"status":"waiting","last_user_text":"","last_assistant_text":"","transcript_path":cwd.join("new.jsonl"),"resume_hint":"","pane":null})).unwrap();
        let mut earlier = session.clone();
        earlier.id = "claude_code:old".into();
        earlier.started_at = 2;
        earlier.transcript_path = cwd.join("old.jsonl");
        // Newest session first, as the scanner supplies them.
        let assigned = assign(&[old, fresh], &[session, earlier]);
        assert_eq!(
            assigned.iter().find(|(id, _)| id == "r2").unwrap().1.id,
            "claude_code:new"
        );
        assert_eq!(
            assigned.iter().find(|(id, _)| id == "r1").unwrap().1.id,
            "claude_code:old"
        );
    }
    #[test]
    fn replaced_bound_transcript_can_move_to_a_new_session_file() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path();
        let run:AgentRun=serde_json::from_value(json!({"id":"r1","server_start":1,"pane_id":"%0","agent":"claude","agent_pid":1,"agent_session_id":null,"cwd":cwd,"project_root":null,"started_at":1000,"ended_at":null,"state":"waiting","state_since":1000,"state_source":"transcript","read_at":null,"last_assistant_text":"","transcript_path":cwd.join("old.jsonl")})).unwrap();
        let session:Session=serde_json::from_value(json!({"id":"claude_code:new","agent":"claude_code","cwd":cwd,"project_root":cwd,"title":"new","started_at":4,"last_activity_at":5,"status":"working","last_user_text":"","last_assistant_text":"","transcript_path":cwd.join("new.jsonl"),"resume_hint":"","pane":null})).unwrap();
        let assigned = assign(&[run], &[session]);
        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].1.id, "claude_code:new");
    }
}
