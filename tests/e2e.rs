use crate::support::BoundedCommand;
macro_rules! e2e_test {
    ($name:ident, $body:block) => {
        e2e_test!($name, false, $body);
    };
    ($name:ident, $stress:expr, $body:block) => {
        #[test]
        fn $name() {
            crate::support::watchdog::run(
                concat!(module_path!(), "::", stringify!($name)),
                move || {
                    let _load = crate::support::load::enter($stress);
                    $body
                },
            );
        }
    };
}
macro_rules! until {
    ($condition:expr) => {
        crate::support::wait_for(stringify!($condition), $condition)
    };
}
#[path = "support/hooks.rs"]
mod hooks;
#[path = "support/hooks_identity.rs"]
mod hooks_identity;
#[path = "support/review.rs"]
mod review;
mod support;
use support::TmuxGuard;
use tmux_sidecar::seams::Tmux;
fn enabled() -> bool {
    if std::env::var("TMUX_SIDECAR_E2E").as_deref() == Ok("1") {
        true
    } else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI requires TMUX_SIDECAR_E2E=1"
        );
        use std::io::Write;
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = writeln!(
                std::io::stderr().lock(),
                "SKIPPED REAL TMUX E2E: run scripts/accept.sh or set TMUX_SIDECAR_E2E=1"
            );
        });
        false
    }
}
e2e_test!(real_adapter_sees_isolated_pane, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let panes = guard.adapter.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].pane_id, "%0");
    assert!(panes[0].pane_pid > 0);
    assert!(panes[0].server_start > 0);
});
e2e_test!(dropping_guard_kills_server, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut check = guard.command();
    assert!(
        guard
            .command()
            .arg("list-sessions")
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
    drop(guard);
    assert!(
        !check
            .arg("list-sessions")
            .bounded_output()
            .unwrap()
            .status
            .success()
    );
});

#[path = "support/lifecycle.rs"]
mod lifecycle;

#[path = "support/protocol.rs"]
mod protocol;

#[path = "support/resilience.rs"]
mod resilience;
#[path = "support/round1.rs"]
mod round1;

#[path = "support/shutdown.rs"]
mod shutdown;

#[path = "support/cli_verbs.rs"]
mod cli_verbs;
#[path = "support/focus.rs"]
mod focus;
#[path = "support/ordering.rs"]
mod ordering;
#[path = "support/selector_groups.rs"]
mod selector_groups;
#[path = "support/spool_load.rs"]
mod spool_load;
#[path = "support/state.rs"]
mod state;
#[path = "support/transcript.rs"]
mod transcript;
#[path = "support/transcript_poll.rs"]
mod transcript_poll;

#[path = "support/cli_deadlines.rs"]
mod cli_deadlines;
#[path = "support/cli_stream.rs"]
mod cli_stream;
#[path = "support/mcp.rs"]
mod mcp;
#[path = "support/mcp_client.rs"]
mod mcp_client;
#[path = "support/mcp_contract.rs"]
mod mcp_contract;
#[path = "support/mcp_daemon.rs"]
mod mcp_daemon;
#[path = "support/mcp_deadlines.rs"]
mod mcp_deadlines;
#[path = "support/mcp_lifecycle.rs"]
mod mcp_lifecycle;
#[path = "support/mcp_replies.rs"]
mod mcp_replies;
#[path = "support/mcp_rpc.rs"]
mod mcp_rpc;

#[path = "support/ui.rs"]
mod ui;
#[path = "support/ui_lifecycle.rs"]
mod ui_lifecycle;

#[path = "support/codex_app_server.rs"]
mod codex_app_server;
#[path = "support/connection_limits.rs"]
mod connection_limits;
#[path = "support/daemon_round2.rs"]
mod daemon_round2;
#[path = "support/history_recovery.rs"]
mod history_recovery;
#[path = "support/review_a2.rs"]
mod review_a2;
#[path = "support/round2_hooks.rs"]
mod round2_hooks;
#[path = "support/skeptic_state.rs"]
mod skeptic_state;
#[path = "support/skeptic_ux.rs"]
mod skeptic_ux;
#[path = "support/ui_stale.rs"]
mod ui_stale;

#[path = "support/binary_recovery.rs"]
mod binary_recovery;
#[path = "support/round3_daemon.rs"]
mod round3_daemon;
#[path = "support/round3_send.rs"]
mod round3_send;
#[path = "support/round3_transcripts.rs"]
mod round3_transcripts;

#[path = "support/notify_episodes.rs"]
mod notify_episodes;
