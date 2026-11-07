//! Bounded fake daemon handshake shared by MCP lifecycle and contract tests.
use crate::support::TmuxGuard;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    time::{Duration, Instant},
};
pub fn listen(guard: &TmuxGuard) -> UnixListener {
    let listener = UnixListener::bind(guard.scratch.path().join("run/sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    listener
}
pub fn accept(listener: &UnixListener, key: &str) -> BufReader<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let (stream, _) = loop {
        match listener.accept() {
            Ok(stream) => break stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "fake daemon accept deadline");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("{e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut stream = BufReader::new(stream);
    let hello = read(&mut stream);
    assert_eq!(hello["method"], "hello");
    writeln!(
        stream.get_mut(),
        "{}",
        json!({"jsonrpc":"2.0","id":hello["id"],"result":{"server_key":key,"rpc_version":1}})
    )
    .unwrap();
    stream
}
pub fn read(stream: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    stream.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
