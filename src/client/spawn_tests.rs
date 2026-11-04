use super::*;
fn until(mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !f() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct KillOnDrop(u32);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        crate::proc::terminate(self.0);
    }
}
fn helper(root: &Path) -> (StartingChild, KillOnDrop) {
    let child = Command::new("/bin/sh").args(["-c", "trap 'echo term > \"$1/term\"; exit 0' TERM; echo ready > \"$1/ready\"; while :; do :; done", "fixture"])
        .arg(root).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let guard = KillOnDrop(child.id());
    let starting = StartingChild {
        child: Some(child),
        socket: root.join("sock"),
    };
    until(|| root.join("ready").exists());
    (starting, guard)
}
#[test]
fn cancellation_before_bind_sends_term_instead_of_kill() {
    let root = tempfile::tempdir().unwrap();
    let (starting, _guard) = helper(root.path());
    drop(starting);
    until(|| root.path().join("term").exists());
}
#[test]
fn bound_socket_hands_child_over_without_killing_it() {
    let root = tempfile::tempdir().unwrap();
    let (starting, guard) = helper(root.path());
    let _listener = std::os::unix::net::UnixListener::bind(root.path().join("sock")).unwrap();
    drop(starting);
    assert!(crate::proc::alive(guard.0 as i32));
    assert!(!root.path().join("term").exists());
    drop(guard);
    until(|| root.path().join("term").exists());
}
