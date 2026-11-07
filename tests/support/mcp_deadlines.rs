//! Pin the MCP wait cap while sharing M4's ordinary/send client budgets.
use super::{TmuxGuard, enabled, mcp_client::Mcp, mcp_daemon};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    time::{Duration, Instant},
};

fn probe(
    tool: &str,
    method: &str,
    args: Value,
    reply_after: Option<Duration>,
) -> (Value, Duration) {
    let guard = TmuxGuard::start();
    let listener = mcp_daemon::listen(&guard);
    let key = tmux_sidecar::paths::server_key(&guard.socket()).unwrap();
    let expected = method.to_string();
    let worker = std::thread::spawn(move || {
        let mut stream = mcp_daemon::accept(&listener, &key);
        assert_eq!(mcp_daemon::read(&mut stream)["method"], expected);
        stream
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(7)))
            .unwrap();
        if let Some(delay) = reply_after {
            std::thread::sleep(delay);
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"jsonrpc":"2.0","id":1,"result":if expected == "attention" { json!([]) } else { json!({}) }})
            )
            .unwrap();
        }
        let mut line = String::new();
        assert_eq!(
            stream.read_line(&mut line).unwrap(),
            0,
            "RPC stream must be released"
        );
    });
    let mut mcp = Mcp::start(&guard, true);
    let start = Instant::now();
    let result = mcp.call(tool, args);
    let elapsed = start.elapsed();
    worker.join().unwrap();
    (result, elapsed)
}

e2e_test!(
    mcp_wait_caps_unresponsive_daemon_at_requested_timeout_plus_two,
    {
        if !enabled() {
            return;
        }
        let (result, elapsed) = probe(
            "wait_for_state",
            "wait",
            json!({"selector":"r42","state":"waiting","timeout":0}),
            None,
        );
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["content"][0]["text"],
            "MCP wait exceeded 2 s (-32004)"
        );
        assert!(elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(4));
    }
);

e2e_test!(mcp_send_shares_longer_cli_response_budget, {
    if !enabled() {
        return;
    }
    let (result, elapsed) = probe(
        "send_prompt",
        "send",
        json!({"selector":"r42","text":"hello"}),
        Some(Duration::from_millis(3200)),
    );
    assert_ne!(result["isError"], true, "{result}");
    assert!(elapsed >= Duration::from_millis(3200));
});

e2e_test!(mcp_read_tools_share_ordinary_cli_response_budget, {
    if !enabled() {
        return;
    }
    let (result, _) = probe(
        "attention",
        "attention",
        json!({}),
        Some(Duration::from_millis(2200)),
    );
    assert_ne!(result["isError"], true, "{result}");
});

e2e_test!(
    mcp_send_overrun_reports_unknown_outcome_instead_of_unreachable,
    {
        if !enabled() {
            return;
        }
        let (result, elapsed) = probe(
            "send_prompt",
            "send",
            json!({"selector":"r42","text":"hello"}),
            None,
        );
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["content"][0]["text"],
            "send outcome unknown — check the pane before retrying (-32001)"
        );
        assert!(elapsed >= Duration::from_secs(5) && elapsed < Duration::from_secs(7));
    }
);
