use super::lifecycle::{list, pid, stop};
use super::{TmuxGuard, enabled};
use crate::support::BoundedCommand;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    time::Duration,
};
e2e_test!(separate_servers_and_rpc_contract, {
    if !enabled() {
        return;
    }
    let a = TmuxGuard::start();
    let b = TmuxGuard::start();
    let mut via_env = std::process::Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
    a.isolate(&mut via_env);
    via_env
        .env("TMUX", format!("{},123,0", a.socket().display()))
        .args(["list", "--json"]);
    let first = super::lifecycle::ok(via_env.bounded_output().unwrap());
    let second = list(&b);
    assert_ne!(first[0]["agent_pid"], second[0]["agent_pid"]);
    assert_ne!(pid(&a), pid(&b));
    assert_ne!(a.scratch.path(), b.scratch.path());
    for guard in [&a, &b] {
        assert!(guard.scratch.path().join("run/sock").exists());
        assert!(guard.scratch.path().join("state/tmux-sidecar.db").exists());
    }
    let stream = crate::support::socket::connect(a.scratch.path().join("run/sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    for (request, code) in [
        ("not json", -32700),
        (r#"{"jsonrpc":"1.0","id":4,"method":"list"}"#, -32600),
        (
            r#"{"jsonrpc":"2.0","id":5,"method":"list","params":{"state":"bogus"}}"#,
            -32602,
        ),
    ] {
        writeln!(writer, "{request}").unwrap();
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read RPC response within socket deadline");
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["error"]["code"], code);
    }
    for method in tmux_sidecar::rpc::METHODS
        .iter()
        .filter(|m| !["list", "daemon.stop"].contains(m))
    {
        writeln!(
            writer,
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":method})
        )
        .unwrap();
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read RPC response within socket deadline");
        let response: Value = serde_json::from_str(&line).unwrap();
        if ["hello", "daemon.status", "subscribe"].contains(method) {
            assert!(response["result"].is_object());
        } else if ["attention", "group.ls"].contains(method) {
            assert!(response["result"].is_array());
        } else if ["signal", "ack", "history", "name", "group.add", "group.rm"].contains(method) {
            assert_eq!(response["error"]["code"], -32602);
        } else {
            assert_eq!(response["error"]["code"], -32601);
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("not implemented yet (M")
            );
        }
    }
    let filtered = super::lifecycle::ok(
        a.sidecar()
            .args(["list", "--state", "done", "--json"])
            .bounded_output()
            .unwrap(),
    );
    assert!(filtered.as_array().unwrap().is_empty());
    let group = a
        .sidecar()
        .args(["list", "--group", "test"])
        .bounded_output()
        .unwrap();
    assert!(group.status.success());
    drop(writer);
    drop(reader);
    stop(&a);
    stop(&b);
});
