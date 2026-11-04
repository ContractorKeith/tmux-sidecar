mod copy_fixture;
mod fake_binary;
pub mod load;
mod process;
pub mod socket;
pub mod watchdog;
pub use process::{BoundedCommand, ChildGuard, output, stage, wait_for};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
};
use tempfile::TempDir;
use tmux_sidecar::tmux::{RealTmux, Socket};

pub struct TmuxGuard {
    pub adapter: RealTmux,
    pub scratch: TempDir,
    path: OsString,
    server_socket: PathBuf,
}
impl TmuxGuard {
    pub fn start() -> Self {
        let scratch = tempfile::Builder::new().prefix("tsx-").tempdir().unwrap();
        let name = scratch
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        // Construct the guard before any operation that could start a server.
        let mut guard = Self {
            adapter: RealTmux {
                socket: Socket::Name(name),
            },
            scratch,
            path: OsString::new(),
            server_socket: PathBuf::new(),
        };
        watchdog::register(guard.scratch.path());
        for dir in ["home", "run", "state", "config", "bin"] {
            fs::create_dir(guard.scratch.path().join(dir)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        for dir in ["run", "state"] {
            fs::set_permissions(
                guard.scratch.path().join(dir),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        let helper = guard.scratch.path().join("fake-agent");
        fake_binary::write(&helper);
        for agent in ["claude", "codex", "grok", "opencode"] {
            fs::copy(&helper, guard.scratch.path().join("bin").join(agent)).unwrap();
        }
        let mut paths = vec![guard.scratch.path().join("bin")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        guard.path = std::env::join_paths(paths).unwrap();
        let mut shell = Command::new("/bin/sh");
        guard.isolate(&mut shell);
        let output = shell
            .args([
                "-c",
                "for agent in claude codex grok opencode; do command -v \"$agent\" || exit; done",
            ])
            .bounded_output()
            .unwrap();
        assert!(output.status.success());
        let resolved = String::from_utf8(output.stdout).unwrap();
        let expected: Vec<_> = ["claude", "codex", "grok", "opencode"]
            .iter()
            .map(|a| {
                guard
                    .scratch
                    .path()
                    .join("bin")
                    .join(a)
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(resolved.lines().collect::<Vec<_>>(), expected);
        // Exercise a copied fake and ensure direct children are killed/reaped on failure too.
        let mut command = Command::new(guard.scratch.path().join("bin/codex"));
        guard.isolate(&mut command);
        let _agent = ChildGuard(spawn_fixture(
            command.stdout(Stdio::null()).stderr(Stdio::null()),
        ));
        let output = guard
            .command()
            .args(["-f", "/dev/null", "new-session", "-d", "-s", "fixture"])
            .arg(guard.scratch.path().join("bin/claude"))
            .arg("--")
            .bounded_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let socket = guard
            .command()
            .args(["display", "-p", "#{socket_path}\n#{pid}"])
            .bounded_output()
            .unwrap();
        assert!(socket.status.success());
        let text = String::from_utf8(socket.stdout).unwrap();
        let mut fields = text.lines();
        let socket = PathBuf::from(fields.next().unwrap());
        watchdog::server(
            guard.scratch.path(),
            fields.next().unwrap().parse().unwrap(),
        );
        guard.server_socket = tmux_sidecar::paths::canonical_socket(&socket).unwrap();
        wait_for("fake Claude startup", || {
            guard.scratch.path().join("state/ready-claude").exists()
        });
        guard
    }
    pub fn isolate(&self, command: &mut Command) {
        let root = self.scratch.path();
        command
            .env("HOME", root.join("home"))
            .env("CLAUDE_CONFIG_DIR", root.join("home/.claude"))
            .env("CODEX_HOME", root.join("home/.codex"))
            .env("GROK_HOME", root.join("home/.grok"))
            .env("OPENCODE_DB", root.join("home/opencode.db"))
            .env("XDG_DATA_HOME", root.join("home/.local/share"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("TMUX_SIDECAR_RUNTIME_DIR", root.join("run"))
            .env("TMUX_SIDECAR_STATE_DIR", root.join("state"))
            .env("PATH", &self.path)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE");
    }
    pub fn command(&self) -> Command {
        let mut command = self.adapter.command();
        self.isolate(&mut command);
        command
    }
}
impl TmuxGuard {
    pub fn socket(&self) -> PathBuf {
        self.server_socket.clone()
    }
    pub fn sidecar(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
        self.isolate(&mut cmd);
        cmd.env("TMUX_SIDECAR_POLL_MS", "100")
            .arg("--tmux-socket")
            .arg(self.socket());
        cmd
    }
}
impl Drop for TmuxGuard {
    fn drop(&mut self) {
        // Scoped to this scratch directory, including cleanup on an assertion failure.
        if self.scratch.path().join("run/daemon.pid").exists() {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_tmux-sidecar"));
            self.isolate(&mut cmd);
            cmd.arg("--tmux-socket")
                .arg(&self.server_socket)
                .args(["daemon", "stop"]);
            let _ = process::try_output(&mut cmd);
        }
        let _ = process::try_output(self.command().arg("kill-server"));
        watchdog::unregister(self.scratch.path());
    }
}
// Concurrent forks can briefly inherit a copy's writable fd before exec closes it.
fn spawn_fixture(command: &mut Command) -> Child {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match command.spawn() {
            Ok(child) => return child,
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("fake agent startup: {error}"),
        }
    }
}
