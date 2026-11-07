//! Copied executable retains an actual agent comm while firing fixture hooks.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    for key in [
        "HOME",
        "TMUX_TMPDIR",
        "TMUX_SIDECAR_RUNTIME_DIR",
        "TMUX_SIDECAR_STATE_DIR",
    ] {
        command.env(
            key,
            std::env::var_os(key).expect("fake agent requires isolated roots"),
        );
    }
    command
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--hook-burst") {
        burst(&args[2], &args[3]);
        return;
    }
    let root = PathBuf::from(std::env::var_os("TMUX_SIDECAR_STATE_DIR").unwrap());
    let pid = std::process::id();
    if let Some(cwd) = args.iter().find_map(|a| a.strip_prefix("--cwd=")) {
        std::env::set_current_dir(cwd).unwrap();
    }
    let _host = if args
        .iter()
        .any(|a| a == "--plugin-host" || a == "--app-server")
    {
        let child = ChildGuard(
            command(std::env::current_exe().unwrap())
                .arg(if args.iter().any(|a| a == "--app-server") {
                    "app-server"
                } else {
                    "--host-child"
                })
                .spawn()
                .unwrap(),
        );
        fs::write(root.join("plugin-host-pid"), child.0.id().to_string()).unwrap();
        Some(child)
    } else {
        None
    };
    let executable = std::env::current_exe().unwrap();
    let name = executable.file_name().unwrap().to_str().unwrap();
    fs::write(root.join(format!("ready-{name}")), b"").unwrap();
    if root.join("capture-input").exists() {
        let mut terminal = ChildGuard(command("stty").args(["raw", "-echo"]).spawn().unwrap());
        terminal.0.wait().unwrap();
        use std::io::Write;
        std::io::stdout().write_all(b"\x1b[?2004h").unwrap();
        std::io::stdout().flush().unwrap();
        let input_root = root.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut input = std::io::stdin().lock();
            let mut file = fs::File::create(input_root.join(format!("input-{pid}"))).unwrap();
            let mut bytes = [0; 4096];
            while let Ok(count) = input.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                file.write_all(&bytes[..count]).unwrap();
                file.flush().unwrap();
            }
        });
    }
    loop {
        let request = root.join(format!("fire-{pid}"));
        if let Ok(text) = fs::read_to_string(&request) {
            fs::remove_file(&request).unwrap();
            let mut lines = text.lines();
            let binary = lines.next().unwrap();
            let format = lines.next().unwrap();
            let fixture = lines.next().unwrap();
            let pane_override = lines.next();
            if format == "burst" {
                burst(binary, fixture);
                fs::write(root.join(format!("fired-{pid}")), "true\n0\n0\n0").unwrap();
                continue;
            }
            let start = std::time::Instant::now();
            let mut command = command(binary);
            if let Some(pane) = pane_override {
                command.env("TMUX_PANE", pane);
            }
            command
                .args(["hook", format])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if format == "codex-notify" {
                command.arg(fs::read_to_string(fixture).unwrap());
            } else {
                command.stdin(fs::File::open(fixture).unwrap());
            }
            let mut child = ChildGuard(command.spawn().unwrap());
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                assert!(start.elapsed() < Duration::from_secs(2));
                std::thread::sleep(Duration::from_millis(1));
            };
            let output = child.0.stdout.take().unwrap();
            let error = child.0.stderr.take().unwrap();
            use std::io::Read;
            let mut stdout = String::new();
            let mut stderr = String::new();
            std::io::BufReader::new(output)
                .read_to_string(&mut stdout)
                .unwrap();
            std::io::BufReader::new(error)
                .read_to_string(&mut stderr)
                .unwrap();
            fs::write(
                root.join(format!("fired-{pid}.new")),
                format!(
                    "{}\n{}\n{}\n{}",
                    status.success(),
                    start.elapsed().as_millis(),
                    stdout.len(),
                    stderr.len()
                ),
            )
            .unwrap();
            fs::rename(
                root.join(format!("fired-{pid}.new")),
                root.join(format!("fired-{pid}")),
            )
            .unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn burst(binary: &str, fixture: &str) {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(200));
    std::thread::scope(|scope| {
        for _ in 0..200 {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                let mut child = ChildGuard(
                    command(binary)
                        .args(["hook", "claude"])
                        .stdin(fs::File::open(fixture).unwrap())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                loop {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        assert!(status.success());
                        break;
                    }
                    assert!(std::time::Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
        }
    });
}
