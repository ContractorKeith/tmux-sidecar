use crate::support::{ChildGuard, TmuxGuard};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{ChildStdin, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

pub struct Mcp {
    // The guard owns the process before any pipe operation can fail.
    _child: ChildGuard,
    input: ChildStdin,
    output: Receiver<Value>,
    next: u64,
}
impl Mcp {
    pub fn start(guard: &TmuxGuard, allow_send: bool) -> Self {
        let mut command = guard.sidecar();
        command.arg("mcp");
        if allow_send {
            command.arg("--allow-send");
        }
        let mut child = ChildGuard(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let input = child.0.stdin.take().unwrap();
        let stdout = child.0.stdout.take().unwrap();
        let (send, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let value = serde_json::from_str(&line.unwrap()).unwrap();
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            _child: child,
            input,
            output,
            next: 0,
        };
        let result = client.request(
            "initialize",
            json!({"protocolVersion":"2024-11-05",
            "capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        );
        assert!(result["capabilities"]["tools"].is_object(), "{result}");
        writeln!(
            client.input,
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(
            self.input,
            "{}",
            json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params})
        )
        .unwrap();
        let value = self
            .output
            .recv_timeout(Duration::from_secs(8))
            .expect("MCP response deadline");
        assert_eq!(value["id"], self.next, "{value}");
        assert!(value.get("error").is_none(), "{value}");
        value["result"].clone()
    }
    pub fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))
    }
    pub fn json(&mut self, name: &str, args: Value) -> Value {
        let result = self.call(name, args);
        assert_ne!(result["isError"], true, "{result}");
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
