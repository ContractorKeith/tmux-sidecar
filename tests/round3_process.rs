use std::{
    os::unix::process::CommandExt,
    process::{Command, Stdio},
};
use tmux_sidecar::{proc::PsProcessTree, seams::ProcessTree};
mod process {
    pub struct ChildGuard(pub std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[test]
fn agent_argv0_is_identity_even_when_comm_and_exe_differ() {
    let child = process::ChildGuard(
        Command::new("/bin/sleep")
            .arg0("grok")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let snapshot = PsProcessTree.snapshot().unwrap();
        if snapshot
            .get(&child.0.id())
            .is_some_and(|p| p.comm == "grok")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "argv0 grok was not recognized"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test]
fn symlinked_agent_with_a_different_executable_name_is_detected() {
    let root = tempfile::tempdir().unwrap();
    let grok = root.path().join("grok");
    std::os::unix::fs::symlink("/bin/sleep", &grok).unwrap();
    let child = process::ChildGuard(
        Command::new(&grok)
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let snapshot = PsProcessTree.snapshot().unwrap();
        if snapshot
            .get(&child.0.id())
            .is_some_and(|p| p.comm == "grok")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "symlink grok was not recognized"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
