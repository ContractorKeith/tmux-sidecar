mod cleanup;
mod copy_fixture;
mod scratch;
pub use scratch::Scratch;
mod fake_binary;
pub mod load;
pub mod no_ancestor;
mod notify_config;
mod process;
pub mod socket;
pub mod watchdog;
pub mod workers;
pub use process::{BoundedCommand, ChildGuard, output, stage, wait_for, wait_for_timeout};
use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
};
use tmux_sidecar::tmux::{RealTmux, Socket};

pub struct TmuxGuard {
    pub adapter: RealTmux,
    pub scratch: Scratch,
    path: OsString,
    server_socket: PathBuf,
}
impl TmuxGuard {
    pub fn start() -> Self {
        Self::start_with_input(false)
    }
    pub fn start_capture() -> Self {
        Self::start_with_input(true)
    }
    fn start_with_input(capture: bool) -> Self {
        let scratch = Scratch::new();
        let socket = scratch.path().join("tmux.sock");
        // Construct the guard before any operation that could start a server.
        let mut guard = Self {
            adapter: RealTmux {
                socket: Socket::Path(socket.clone()),
            },
            scratch,
            path: OsString::new(),
            server_socket: socket,
        };
        fs::write(
            guard.scratch.path().join("tmux.socket"),
            guard.server_socket.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        for dir in ["home", "run", "state", "config", "bin"] {
            fs::create_dir(guard.scratch.path().join(dir)).unwrap();
        }
        if capture {
            fs::write(guard.scratch.path().join("state/capture-input"), b"").unwrap();
        }

        notify_config::disable(guard.scratch.path());
        use std::os::unix::fs::PermissionsExt;
        for dir in ["run", "state"] {
            fs::set_permissions(
                guard.scratch.path().join(dir),
                fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        fake_binary::link_agents(&guard.scratch.path().join("bin"));
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
        let _agent = ChildGuard::new(spawn_fixture(
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
        fs::write(
            guard.scratch.path().join("tmux.socket"),
            guard.server_socket.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
        guard.adapter.socket = Socket::Path(guard.server_socket.clone());
        process::wait_for_timeout(
            "fake Claude startup",
            std::time::Duration::from_secs(10),
            || guard.scratch.path().join("state/ready-claude").exists(),
        );
        guard
    }
    pub fn isolate(&self, command: &mut Command) {
        let root = self.scratch.path();
        command
            .env("HOME", root.join("home"))
            .env("TMUX_TMPDIR", root)
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
        cleanup::fixture(
            self.scratch.path(),
            watchdog::server_pid(self.scratch.path()),
            false,
        );
    }
}
// Concurrent forks can briefly inherit a copy's writable fd before exec closes it.
fn spawn_fixture(command: &mut Command) -> Child {
    let deadline =
        std::time::Instant::now() + process::load_timeout(std::time::Duration::from_secs(1));
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
