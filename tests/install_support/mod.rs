use std::{
    fs,
    io::{self, Read},
    path::Path,
    process::{Command, Output},
    time::{Duration, Instant},
};
pub fn put(home: &Path, path: &str, text: &str) {
    let path = home.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
pub fn run(home: &Path, install: bool, chain: bool, dry: bool) -> io::Result<()> {
    tmux_sidecar::install::run(
        home,
        Path::new(env!("CARGO_BIN_EXE_tmux-sidecar")),
        &[],
        install,
        chain,
        dry,
    )
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub fn output(command: &mut Command) -> Output {
    use std::process::Stdio;
    let mut child = ChildGuard(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "child command timed out");
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}
