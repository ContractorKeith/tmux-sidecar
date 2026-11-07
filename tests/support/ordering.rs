use super::lifecycle::{list, stop};
use super::state::rpc;
use super::{TmuxGuard, enabled};
use serde_json::{Value, json};
fn signal(run: &Value, event: &str, at: u64) -> Value {
    json!({"server_start":run["server_start"],"pane_id":run["pane_id"],"agent":run["agent"],"agent_pid":run["agent_pid"],"at":at,"nonce":format!("{at:032x}"),"source":"hook:claude","event":event,"cwd":run["cwd"],"payload":{}})
}
fn streak(guard: &TmuxGuard) -> (Value, u64) {
    let run = list(guard)[0].clone();
    let at = tmux_sidecar::daemon::now() + 100;
    rpc(guard, "signal", signal(&run, "UserPromptSubmit", at));
    rpc(guard, "signal", signal(&run, "PostToolUse", at + 20));
    (run, at)
}
e2e_test!(late_stop_after_same_state_streak_is_ignored_live, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let (run, at) = streak(&guard);
    rpc(&guard, "signal", signal(&run, "Stop", at + 10));
    assert_eq!(list(&guard)[0]["state"], "working");
    assert_eq!(rpc(&guard, "attention", json!({})), json!([]));
});
e2e_test!(
    late_stop_after_same_state_streak_is_ignored_from_leftover_claim,
    {
        if !enabled() {
            return;
        }
        let guard = TmuxGuard::start();
        let (run, at) = streak(&guard);
        stop(&guard);
        std::fs::write(
            guard.scratch.path().join("state/spool.replaying.123"),
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","method":"signal","params":signal(&run,"Stop",at+10)})
            ),
        )
        .unwrap();
        assert_eq!(list(&guard)[0]["state"], "working");
        assert_eq!(rpc(&guard, "attention", json!({})), json!([]));
        let history = rpc(&guard, "history", json!({"sel":run["id"]}));
        assert_eq!(
            history
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["source"] == "hook:claude")
                .count(),
            3
        );
    }
);
