//! The hook caller has a hard deadline independent of stdin, process probes and I/O.
mod delivery;
pub mod signal;
pub mod spool;
use std::{io, time::Duration};

pub fn entry(args: Vec<String>) {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .and_then(|rt| rt.block_on(delivery::run(args)));
        let _ = send.send(result);
    });
    let _ = receive.recv_timeout(Duration::from_millis(180));
}
pub fn log(state: &std::path::Path, error: &dyn std::fmt::Display) -> io::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::OpenOptionsExt,
    };
    let path = state.join("hook.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() >= 1024 * 1024) {
        fs::rename(&path, state.join("hook.log.1"))?;
    }
    writeln!(
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?,
        "{error}"
    )
}
