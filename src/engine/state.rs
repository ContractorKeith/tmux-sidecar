//! State transitions depend only on evidence and the supplied clock.
use crate::model::{AgentKind, AgentRun, AgentState};
use AgentKind::*;
use AgentState::*;

pub enum Evidence<'a> {
    Hook {
        event: &'a str,
        detail: Option<&'a str>,
        at: u64,
        last_signal_at: Option<u64>,
    },
    Transcript {
        state: AgentState,
        has_hooks: bool,
        at: u64,
    },
    Tick {
        idle_after_ms: u64,
    },
    Exit,
}
pub fn transition(run: &AgentRun, evidence: Evidence<'_>, now: u64) -> (AgentState, bool) {
    let next = if matches!(evidence, Evidence::Exit) || run.ended_at.is_some() {
        Done
    } else {
        match evidence {
            Evidence::Hook {
                event,
                detail,
                at,
                last_signal_at,
            } if last_signal_at.is_none_or(|latest| at >= latest) => {
                hook(run.agent, event, detail).unwrap_or(run.state)
            }
            Evidence::Transcript {
                state,
                has_hooks: false,
                at,
            } => {
                // Re-reading the same completed turn must not undo clock-derived idle.
                if run.state == Idle
                    && run.state_source == "clock"
                    && state == Waiting
                    && at <= run.state_since
                {
                    Idle
                } else {
                    state
                }
            }
            Evidence::Tick { idle_after_ms }
                if run.state == Waiting
                    && run.read_at.is_some_and(|at| at >= run.state_since)
                    && now.saturating_sub(run.state_since) > idle_after_ms =>
            {
                Idle
            }
            _ => run.state,
        }
    };
    (next, next != run.state)
}
fn hook(agent: AgentKind, event: &str, detail: Option<&str>) -> Option<AgentState> {
    match (agent, event, detail) {
        (Claude | Grok | Codex, "UserPromptSubmit" | "PreToolUse" | "PostToolUse", _) => {
            Some(Working)
        }
        (Claude | Grok, "Notification", Some("permission_prompt"))
        | (Claude | Grok | Codex, "PermissionRequest", _) => Some(Blocked),
        (Claude | Grok, "Stop", _)
        | (Grok, "StopCancelled", _)
        | (Claude | Grok, "Notification", Some("idle_prompt"))
        | (Codex, "agent-turn-complete", _)
        | (Codex, "Interrupt", _) => Some(Waiting),
        (Claude | Grok, "StopFailure", _) => Some(Error),
        (Claude | Grok | Codex, "SessionEnd", _) => Some(Done),
        (Opencode, "session.status", Some("busy")) | (Opencode, "permission.replied", _) => {
            Some(Working)
        }
        (Opencode, "permission.asked", _) => Some(Blocked),
        (Opencode, "session.idle", _) => Some(Waiting),
        (Opencode, "session.error", _) => Some(Error),
        _ => None,
    }
}
