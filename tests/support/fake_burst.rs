//! Gate concurrent hook execs with timed sockets, without polling or extra worker threads.
use super::{ChildGuard, command};
use std::{
    fs,
    io::{Read, Write},
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    process::Stdio,
    time::{Duration, Instant},
};
pub fn entry(args: &[String]) -> ! {
    // SAFETY: burst always installs a dedicated UnixStream as this helper's stdin.
    let mut gate = unsafe { UnixStream::from_raw_fd(0) };
    gate.set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    gate.read_exact(&mut [0])
        .expect("hook exec gate timed out or closed");
    drop(gate);
    use std::os::unix::process::CommandExt;
    panic!(
        "hook exec failed: {}",
        command(&args[2])
            .args(["hook", "claude"])
            .stdin(fs::File::open(&args[3]).unwrap())
            .exec()
    );
}
pub fn burst(binary: &str, fixture: &str, count: usize) {
    let mut children = Vec::new();
    let mut gates = Vec::new();
    for _ in 0..count {
        let (release, gate) = UnixStream::pair().unwrap();
        release
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        children.push(ChildGuard(
            command(std::env::current_exe().unwrap())
                .arg("--gated-hook")
                .arg(binary)
                .arg(fixture)
                .stdin(Stdio::from(OwnedFd::from(gate)))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        ));
        gates.push(release);
    }
    for mut gate in gates {
        gate.write_all(&[1]).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let mut done = true;
        for child in &mut children {
            match child.0.try_wait().unwrap() {
                Some(status) => assert!(status.success()),
                None => done = false,
            }
        }
        if done {
            break;
        }
        assert!(Instant::now() < deadline, "hook burst exceeded 20 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}
