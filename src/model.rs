use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
    Grok,
    Opencode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Working,
    Waiting,
    Blocked,
    Idle,
    Error,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunSelector {
    Run(u64),
    Pane(u32),
}

impl FromStr for RunSelector {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = "expected r<run id> or %<pane id>";
        let digits = value.get(1..).ok_or(invalid)?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid);
        }
        match value.as_bytes()[0] {
            b'r' => digits
                .parse()
                .ok()
                .filter(|id| *id > 0)
                .map(Self::Run)
                .ok_or(invalid),
            b'%' => digits.parse().map(Self::Pane).map_err(|_| invalid),
            _ => Err(invalid),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub server_start: u64,
    pub pane_id: String,
    pub agent: AgentKind,
    pub agent_pid: u32,
    pub agent_session_id: Option<String>,
    pub cwd: String,
    pub project_root: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub state: AgentState,
    pub state_since: u64,
    pub state_source: String,
    pub read_at: Option<u64>,
    pub last_assistant_text: String,
    pub transcript_path: Option<String>,
}

impl std::fmt::Display for RunSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run(id) => write!(f, "r{id}"),
            Self::Pane(id) => write!(f, "%{id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_kinds_round_trip() {
        for (kind, name) in [
            (AgentKind::Claude, "claude"),
            (AgentKind::Codex, "codex"),
            (AgentKind::Grok, "grok"),
            (AgentKind::Opencode, "opencode"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<AgentKind>(name.into()).unwrap(),
                kind
            );
        }
        assert_eq!(
            serde_json::to_value(session_board::model::Agent::Codex).unwrap(),
            "codex"
        );
    }
    #[test]
    fn states_round_trip() {
        for (state, name) in [
            (AgentState::Working, "working"),
            (AgentState::Waiting, "waiting"),
            (AgentState::Blocked, "blocked"),
            (AgentState::Idle, "idle"),
            (AgentState::Error, "error"),
            (AgentState::Done, "done"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), name);
            assert_eq!(
                serde_json::from_value::<AgentState>(name.into()).unwrap(),
                state
            );
        }
    }
    #[test]
    fn selectors() {
        assert_eq!("r42".parse(), Ok(RunSelector::Run(42)));
        assert_eq!("%7".parse(), Ok(RunSelector::Pane(7)));
        assert_eq!("%0".parse(), Ok(RunSelector::Pane(0)));
        for bad in [
            "",
            "r",
            "%",
            "42",
            "r0",
            "r-1",
            "%+7",
            "r 2",
            "r18446744073709551616",
            "%4294967296",
            "é7",
        ] {
            assert!(bad.parse::<RunSelector>().is_err(), "{bad}");
        }
    }
}
