//! A fake socket pins the MCP adapter's wait/send contract before M4 is available.
use super::mcp_client::Mcp;
use super::{TmuxGuard, enabled};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    time::{Duration, Instant},
};

e2e_test!(mcp_wait_and_send_each_use_one_rpc_and_preserve_errors, {
    if !enabled() {
        return;
    }
    let guard = TmuxGuard::start();
    let listener = UnixListener::bind(guard.scratch.path().join("run/sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let key = tmux_sidecar::paths::server_key(&guard.socket()).unwrap();
    let worker = std::thread::spawn(move || {
        for (method, expected, response) in [
            (
                "wait",
                json!({"sel":"r42","until":"waiting","timeout":3}),
                json!({"result":{"id":"r42","state":"waiting"}}),
            ),
            (
                "wait",
                json!({"sel":"%7","until":"done","timeout":600}),
                json!({"error":{"code":-32004,"message":"timeout: run r42 is working"}}),
            ),
            (
                "send",
                json!({"sel":"r42","text":"hello"}),
                json!({"result":{}}),
            ),
            (
                "send",
                json!({"sel":"r42","text":"no"}),
                json!({"error":{"code":-32003,"message":"refused: run r42 is working"}}),
            ),
        ] {
            let deadline = Instant::now() + Duration::from_secs(8);
            let (stream, _) = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "socket accept deadline");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            let hello: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(hello["method"], "hello");
            writeln!(stream.get_mut(), "{}", json!({"jsonrpc":"2.0","id":hello["id"],"result":{"server_key":key,"rpc_version":1}})).unwrap();
            line.clear();
            stream.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], method);
            assert_eq!(request["params"], expected);
            if expected["timeout"] == 3 {
                // A valid wait must outlive the ordinary two-second RPC deadline.
                std::thread::sleep(Duration::from_millis(2200));
            }
            let mut response = response;
            response["jsonrpc"] = json!("2.0");
            response["id"] = request["id"].clone();
            writeln!(stream.get_mut(), "{response}").unwrap();
            line.clear();
            assert_eq!(
                stream.read_line(&mut line).unwrap(),
                0,
                "one RPC after hello"
            );
        }
    });
    let mut mcp = Mcp::start(&guard, true);
    assert_eq!(
        mcp.json(
            "wait_for_state",
            json!({"selector":"r42","state":"waiting","timeout":3})
        )["state"],
        "waiting"
    );
    let result = mcp.call("wait_for_state", json!({"selector":"%7","state":"done"}));
    assert_eq!(result["isError"], true);
    assert!(result.to_string().contains("-32004"));
    assert_eq!(
        mcp.json("send_prompt", json!({"selector":"r42","text":"hello"})),
        json!({})
    );
    let result = mcp.call("send_prompt", json!({"selector":"r42","text":"no"}));
    assert_eq!(result["isError"], true);
    assert!(result.to_string().contains("refused: run r42 is working"));
    assert!(result.to_string().contains("-32003"));
    worker.join().unwrap();
});
