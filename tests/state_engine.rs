use AgentState::*;
use tmux_sidecar::{
    engine::{Evidence, transition},
    hook::signal,
    model::{AgentKind, AgentRun, AgentState},
};
fn run(agent: AgentKind) -> AgentRun {
    AgentRun {
        id: "r1".into(),
        server_start: 1,
        pane_id: "%0".into(),
        agent,
        agent_pid: 10,
        agent_session_id: None,
        cwd: "/work".into(),
        project_root: None,
        started_at: 10,
        ended_at: None,
        state: Idle,
        state_since: 100,
        state_source: "process".into(),
        read_at: None,
        last_assistant_text: String::new(),
        transcript_path: None,
    }
}
#[test]
fn every_plan_hook_row_from_real_and_synthesized_fixtures() {
    use AgentKind::*;
    for (agent, dir) in [
        (Claude, "claude"),
        (Grok, "grok"),
        (Codex, "codex"),
        (Opencode, "opencode"),
    ] {
        for (file, expected) in match agent {
            Claude => vec![
                ("UserPromptSubmit", Working),
                ("PreToolUse", Working),
                ("PostToolUse", Working),
                ("PermissionRequest", Blocked),
                ("Notification-permission_prompt", Blocked),
                ("Notification-idle_prompt", Waiting),
                ("Stop", Waiting),
                ("StopFailure", Error),
                ("SessionEnd", Done),
            ],
            Grok => vec![
                ("UserPromptSubmit", Working),
                ("PermissionRequest", Blocked),
                ("PreToolUse", Working),
                ("PostToolUse", Working),
                ("Notification-permission_prompt", Blocked),
                ("Notification-idle_prompt", Waiting),
                ("Stop", Waiting),
                ("StopCancelled", Waiting),
                ("StopFailure", Error),
                ("SessionEnd", Done),
            ],
            Codex => vec![
                ("UserPromptSubmit", Working),
                ("PreToolUse", Working),
                ("PostToolUse", Working),
                ("PermissionRequest", Blocked),
                ("notify", Waiting),
                ("Interrupt", Waiting),
                ("SessionEnd", Done),
            ],
            Opencode => vec![
                ("session.status", Working),
                ("permission.asked", Blocked),
                ("permission.replied", Working),
                ("session.idle", Waiting),
                ("session.error", Error),
            ],
        } {
            let payload =
                std::fs::read_to_string(format!("tests/fixtures/hooks/{dir}/{file}.json")).unwrap();
            let parsed = signal::parse(
                if file == "notify" {
                    "codex-notify"
                } else {
                    dir
                },
                &serde_json::from_str(&payload).unwrap(),
            )
            .unwrap();
            let current = run(agent);
            let result = transition(
                &current,
                Evidence::Hook {
                    event: &parsed.event,
                    detail: parsed.detail.as_deref(),
                    at: 101,
                    last_signal_at: Some(current.state_since),
                },
                101,
            );
            assert_eq!(result, (expected, true), "{dir}/{file}");
        }
    }
}
#[path = "support/state_precedence.rs"]
mod precedence;
