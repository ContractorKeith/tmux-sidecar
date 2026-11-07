use super::{TmuxGuard, enabled, mcp_client::Mcp, mcp_daemon};
use serde_json::json;
use std::{
    io::BufRead,
    sync::mpsc,
    time::{Duration, Instant},
};

fn pending_wait(eof: bool) {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let listener = mcp_daemon::listen(&guard);
    let key = tmux_sidecar::paths::server_key(&guard.socket()).unwrap();
    let (ready, receive_ready) = mpsc::channel();
    let (closed, receive_closed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut stream = mcp_daemon::accept(&listener, &key);
        assert_eq!(mcp_daemon::read(&mut stream)["method"], "wait");
        ready.send(()).unwrap();
        let mut line = String::new();
        let result = stream.read_line(&mut line);
        let _ = closed.send(result);
    });
    let mut mcp = Mcp::start(&guard, false);
    let id = mcp.begin(
        "tools/call",
        json!({"name":"wait_for_state","arguments":{"selector":"r42","state":"waiting"}}),
    );
    receive_ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let start = Instant::now();
    if eof {
        mcp.eof();
    } else {
        mcp.notify_frame(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"test cancellation"}}));
    }
    assert_eq!(
        receive_closed
            .recv_timeout(Duration::from_secs(1))
            .expect("cancel must close RPC within 1 s")
            .unwrap(),
        0
    );
    if eof {
        while !mcp.exited() {
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "stdin EOF exit deadline"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    } else {
        assert!(mcp.request("tools/list", json!({}))["tools"].is_array());
    }
    worker.join().unwrap();
}
e2e_test!(mcp_cancellation_releases_pending_rpc, {
    pending_wait(false);
});
e2e_test!(mcp_stdin_eof_releases_pending_rpc_and_exits, {
    pending_wait(true);
});

e2e_test!(mcp_rejects_arguments_before_opening_daemon_socket, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let listener = mcp_daemon::listen(&guard);
    let mut mcp = Mcp::start(&guard, true);
    for (tool, args, message) in [
        (
            "wait_for_state",
            json!({"selector":"r42","state":"waiting","timeout":601}),
            "invalid timeout: 601",
        ),
        (
            "wait_for_state",
            json!({"selector":"r42","state":"waiting","timeout":-1}),
            "invalid timeout: -1",
        ),
        (
            "wait_for_state",
            json!({"selector":"r42","state":"working"}),
            r#"invalid state: "working""#,
        ),
        (
            "wait_for_state",
            json!({"selector":"bad","state":"done"}),
            "expected r<run id> or %<pane id>",
        ),
        (
            "wait_for_state",
            json!({"selector":"r42"}),
            r#"missing "state""#,
        ),
        (
            "list_runs",
            json!({"force":true}),
            "unexpected parameter: force",
        ),
        (
            "send_prompt",
            json!({"selector":"r42","text":"no","force":true}),
            "unexpected parameter: force",
        ),
    ] {
        let result = mcp.call(tool, args);
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["text"], message);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "invalid input reached daemon"
        );
    }
    let id = mcp.begin("tools/call", json!({"name":"unknown","arguments":{}}));
    let result = mcp.receive();
    assert_eq!(result["id"], id);
    assert_eq!(result["error"]["code"], -32602);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
});

e2e_test!(mcp_daemon_disconnect_has_actionable_message, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let listener = mcp_daemon::listen(&guard);
    let key = tmux_sidecar::paths::server_key(&guard.socket()).unwrap();
    let worker = std::thread::spawn(move || {
        let mut stream = mcp_daemon::accept(&listener, &key);
        assert_eq!(mcp_daemon::read(&mut stream)["method"], "list");
    });
    let mut mcp = Mcp::start(&guard, false);
    assert_eq!(
        mcp.call("list_runs", json!({}))["content"][0]["text"],
        "RPC list connection failed: daemon connection closed (-32001)"
    );
    worker.join().unwrap();
});

e2e_test!(mcp_sdk_ignores_malformed_json_and_continues, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut mcp = Mcp::start(&guard, false);
    // rmcp 3.8 ignores syntactically invalid JSON (rust-sdk#938), rather than
    // emitting parse errors. A subsequent correlated reply proves recovery.
    mcp.raw("{broken JSON");
    assert!(mcp.request("tools/list", json!({}))["tools"].is_array());
});

e2e_test!(mcp_tool_descriptions_keep_shared_states_in_instructions, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let mut mcp = Mcp::start(&guard, true);
    let listed = mcp.request("tools/list", json!({}));
    for tool in listed["tools"].as_array().unwrap() {
        assert!(!tool["description"].as_str().unwrap().contains("States:"));
        for property in tool["inputSchema"]["properties"]
            .as_object()
            .unwrap()
            .values()
        {
            assert!(!property["description"].as_str().unwrap().is_empty());
        }
    }
});
