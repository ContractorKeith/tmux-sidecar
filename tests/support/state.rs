use super::lifecycle::{list, stop};
use super::{TmuxGuard, enabled};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
};
pub(super) fn rpc(guard: &TmuxGuard, method: &str, params: Value) -> Value {
    let stream = crate::support::socket::connect(guard.scratch.path().join("run/sock")).unwrap();
    let mut reader = BufReader::new(stream);
    writeln!(
        reader.get_mut(),
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
    )
    .unwrap();
    let value = read(&mut reader);
    assert!(value.get("error").is_none(), "{method}: {value}");
    value["result"].clone()
}
fn read(reader: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
e2e_test!(fixture_states_attention_history_and_subscription, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let run = list(&guard)[0].clone();
    let pid = run["agent_pid"].as_u64().unwrap();
    let stream = crate::support::socket::connect(guard.scratch.path().join("run/sock")).unwrap();
    let mut subscription = BufReader::new(stream);
    writeln!(
        subscription.get_mut(),
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"subscribe"})
    )
    .unwrap();
    assert_eq!(read(&mut subscription)["result"]["subscribed"], true);
    let mut expected = Vec::new();
    for (fixture, state) in [
        ("UserPromptSubmit", "working"),
        ("Notification-permission_prompt", "blocked"),
        ("PreToolUse", "working"),
        ("Stop", "waiting"),
    ] {
        super::hooks::fire(
            &guard,
            pid,
            "claude",
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/hooks/claude/{fixture}.json")),
        );
        until!(|| list(&guard)[0]["state"] == state);
        let event = read(&mut subscription);
        assert_eq!(event["method"], "state_changed");
        assert_eq!(event["params"]["to"], state);
        expected.push(state);
        if state == "blocked" {
            let queue = super::lifecycle::ok(crate::support::output(
                guard.sidecar().args(["attention", "--json"]),
            ));
            assert_eq!(queue[0]["id"], run["id"]);
            rpc(&guard, "ack", json!({"selectors":[run["id"]]}));
            assert_eq!(rpc(&guard, "attention", json!({})), json!([]));
        }
    }
    assert_eq!(rpc(&guard, "attention", json!({}))[0]["id"], run["id"]);
    let history = rpc(&guard, "history", json!({"sel":run["id"],"limit":100}));
    assert_eq!(
        history
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "state_changed")
            .count(),
        4
    );
    rpc(&guard, "name", json!({"sel":run["id"],"name":"build"}));
    assert!(
        crate::support::output(guard.command().args(["new-window", "-d", "sleep 60"]))
            .status
            .success()
    );
    assert!(
        crate::support::output(guard.command().args(["kill-pane", "-t", "%0"]))
            .status
            .success()
    );
    let event = read(&mut subscription);
    assert_eq!(event["params"]["to"], "done");
    assert_eq!(event["params"]["from"], "waiting");
    drop(subscription);
    stop(&guard);
});
e2e_test!(parallel_stop_start_twenty_cycles, true, {
    if !enabled() {
        return;
    }
    // Twenty independent servers churn concurrently; each gets three complete cycles.
    let (send, receive) = std::sync::mpsc::channel();
    for _ in 0..20 {
        let send = send.clone();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| {
                let guard = TmuxGuard::start();
                for _ in 0..3 {
                    list(&guard);
                    stop(&guard);
                }
            });
            let _ = send.send(outcome);
        });
    }
    drop(send);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(50);
    for _ in 0..20 {
        receive
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap()
            .unwrap_or_else(|e| std::panic::resume_unwind(e));
    }
});
