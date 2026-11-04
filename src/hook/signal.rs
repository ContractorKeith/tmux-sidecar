use crate::model::AgentKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub server_start: u64,
    pub pane_id: String,
    pub agent: AgentKind,
    pub agent_pid: u32,
    pub at: u64,
    pub source: String,
    pub kind: String,
    pub agent_session_id: Option<String>,
    pub cwd: String,
    pub transcript_path: Option<String>,
    pub payload: Value,
}
impl Signal {
    pub fn validate(&self) -> io::Result<()> {
        if self.agent_pid == 0
            || self.server_start == 0
            || self.at > i64::MAX as u64
            || self.server_start > i64::MAX as u64
            || !matches!(self.pane_id.parse(), Ok(crate::model::RunSelector::Pane(_)))
            || self.kind.is_empty()
            || !self.source.starts_with("hook:")
        {
            return Err(io::Error::other("invalid signal identity or kind"));
        }
        Ok(())
    }
}

pub fn parse(
    format: &str,
    payload: Value,
) -> io::Result<(String, Option<String>, String, Option<String>)> {
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| payload.get(*k)?.as_str())
            .map(str::to_owned)
    };
    let event = if matches!(format, "opencode" | "codex-notify") {
        payload.get("type").and_then(Value::as_str)
    } else {
        payload
            .get("hook_event_name")
            .or_else(|| payload.get("hookEventName"))
            .and_then(Value::as_str)
    }
    .ok_or_else(|| io::Error::other("payload has no event name"))?;
    let detail = text(&[
        "notification_type",
        "notificationType",
        "tool_name",
        "toolName",
        "reason",
    ])
    .or_else(|| {
        payload
            .pointer("/properties/status/type")?
            .as_str()
            .map(str::to_owned)
    });
    let kind = match detail {
        Some(detail) => format!("{event}:{detail}"),
        None => event.to_owned(),
    };
    let session = text(&["session_id", "sessionId", "thread-id"]).or_else(|| {
        payload
            .pointer("/properties/sessionID")?
            .as_str()
            .map(str::to_owned)
    });
    Ok((
        kind,
        session,
        text(&["cwd"]).unwrap_or_default(),
        text(&["transcript_path", "transcriptPath"]),
    ))
}
