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
    input: Option<ChildStdin>,
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
        let mut child = ChildGuard::new(
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
            input: Some(input),
            output,
            next: 0,
        };
        let result = client.request(
            "initialize",
            json!({"protocolVersion":"2024-11-05",
            "capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        );
        assert!(result["capabilities"]["tools"].is_object(), "{result}");
        let instructions = result["instructions"].as_str().unwrap();
        for term in [
            "agent run",
            "working",
            "waiting",
            "blocked",
            "idle",
            "error",
            "done",
        ] {
            assert!(
                instructions.contains(term),
                "missing {term}: {instructions}"
            );
        }
        writeln!(
            client.input.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client
    }
    pub fn begin(&mut self, method: &str, params: Value) -> u64 {
        self.next += 1;
        self.notify_frame(json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params}));
        self.next
    }
    pub fn notify_frame(&mut self, frame: Value) {
        writeln!(self.input.as_mut().unwrap(), "{frame}").unwrap();
    }
    pub fn raw(&mut self, text: &str) {
        writeln!(self.input.as_mut().unwrap(), "{text}").unwrap();
    }
    pub fn receive(&self) -> Value {
        self.output
            .recv_timeout(Duration::from_secs(8))
            .expect("MCP response deadline")
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.begin(method, params);
        let value = self.receive();
        assert_eq!(value["id"], id, "{value}");
        assert!(value.get("error").is_none(), "{value}");
        value["result"].clone()
    }
    pub fn eof(&mut self) {
        drop(self.input.take());
    }
    pub fn exited(&mut self) -> bool {
        self._child.0.try_wait().unwrap().is_some()
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
