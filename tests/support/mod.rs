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
        };
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
        // Build the bin-less helper directly: `cargo test --test e2e` also works on a clean tree.
        let result = Command::new("rustc")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_agent.rs"))
            .arg("-o")
            .arg(&helper)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        for agent in ["claude", "codex", "grok", "opencode"] {
            fs::copy(&helper, guard.scratch.path().join("bin").join(agent)).unwrap();
        }
        let mut paths = vec![guard.scratch.path().join("bin")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        guard.path = std::env::join_paths(paths).unwrap();
        let mut shell = Command::new("/bin/sh");
        guard.isolate(&mut shell);
        let output = shell.args(["-c", "command -v claude"]).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            guard.scratch.path().join("bin/claude").to_str().unwrap()
        );
        // Exercise a copied fake and ensure direct children are killed/reaped on failure too.
        let mut command = Command::new(guard.scratch.path().join("bin/codex"));
        guard.isolate(&mut command);
        let _agent = ChildGuard(
            command
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let output = guard
            .command()
            .args(["-f", "/dev/null", "new-session", "-d", "-s", "fixture"])
            .arg(guard.scratch.path().join("bin/claude"))
            .arg("--")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        guard
    }
    pub fn isolate(&self, command: &mut Command) {
        let root = self.scratch.path();
        command
            .env("HOME", root.join("home"))
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
        let output = self
            .command()
            .args(["display", "-p", "#{socket_path}"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().into()
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
                .arg("/unused")
                .args(["daemon", "stop"]);
            let _ = cmd.output();
        }
        let _ = self.command().arg("kill-server").output();
    }
}
pub struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
