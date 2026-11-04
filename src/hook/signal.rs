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
    pub nonce: String,
    pub event: String,
    pub detail: Option<String>,
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
            || self.event.is_empty()
            || self.nonce.len() != 32
            || !self.nonce.bytes().all(|b| b.is_ascii_hexdigit())
            || !self.source.starts_with("hook:")
        {
            return Err(io::Error::other("invalid signal identity or kind"));
        }
        Ok(())
    }
}

pub fn parse(format: &str, payload: &Value) -> io::Result<Parsed> {
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
    let mut detail = text(&[
        "notification_type",
        "notificationType",
        "tool_name",
        "toolName",
        "reason",
    ])
    .or_else(|| {
        payload
            .pointer("/data/status/type")
            .or_else(|| payload.pointer("/properties/status/type"))?
            .as_str()
            .map(str::to_owned)
    });
    let event = match event {
        "session.execution.started" if format == "opencode" => {
            detail = Some("busy".into());
            "session.status"
        }
        "session.execution.succeeded" | "session.execution.interrupted" if format == "opencode" => {
            "session.idle"
        }
        "session.execution.failed" if format == "opencode" => "session.error",
        event => event,
    };
    let session = text(&["session_id", "sessionId", "thread-id"]).or_else(|| {
        payload
            .pointer("/data/sessionID")
            .or_else(|| payload.pointer("/properties/sessionID"))?
            .as_str()
            .map(str::to_owned)
    });
    Ok(Parsed {
        event: event.to_owned(),
        detail,
        session_id: session,
        cwd: text(&["cwd"])
            .or_else(|| {
                payload
                    .pointer("/location/directory")?
                    .as_str()
                    .map(str::to_owned)
            })
            .unwrap_or_default(),
        transcript_path: text(&["transcript_path", "transcriptPath"]),
    })
}

#[derive(Debug)]
pub struct Parsed {
    pub event: String,
    pub detail: Option<String>,
    pub session_id: Option<String>,
    pub cwd: String,
    pub transcript_path: Option<String>,
}
impl Signal {
    pub fn validate_frame(&self) -> io::Result<()> {
        let mut copy = self.clone();
        copy.server_start = copy.server_start.max(1);
        copy.validate()
    }
}
pub fn nonce() -> io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
