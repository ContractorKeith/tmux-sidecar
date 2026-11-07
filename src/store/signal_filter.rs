use crate::{
    hook::signal::Signal,
    model::{AgentKind, AgentRun},
};

pub(super) fn ignore(run: &AgentRun, signal: &Signal) -> bool {
    if signal.agent != AgentKind::Codex || signal.event != "agent-turn-complete" {
        return false;
    }
    if let Some(id) = &run.agent_session_id {
        return signal.agent_session_id.as_ref() != Some(id);
    }
    signal
        .payload
        .get("input-messages")
        .and_then(|v| v.as_array())
        .is_some_and(|messages| {
            messages.iter().filter_map(|v| v.as_str()).any(|text| {
                text.trim_start()
                    .starts_with("Generate a concise, single-line task title")
            })
        })
}
