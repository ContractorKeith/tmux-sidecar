//! Stop owned tmux servers before escalating only their recorded process trees.
use super::{process, watchdog};
use std::{
    collections::BTreeSet,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
fn isolated(program: impl AsRef<std::ffi::OsStr>, root: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env("HOME", root.join("home"))
        .env("TMUX_TMPDIR", root)
        .env("TMUX_SIDECAR_RUNTIME_DIR", root.join("run"))
        .env("TMUX_SIDECAR_STATE_DIR", root.join("state"))
        .env_remove("TMUX")
        .env_remove("TMUX_PANE");
    command
}
fn running(pid: u32) -> bool {
    i32::try_from(pid).is_ok_and(|pid| pid > 1 && tmux_sidecar::proc::running(pid).unwrap_or(false))
}
pub fn terminate(pids: &[u32]) {
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        for &pid in pids.iter().rev().filter(|pid| running(**pid)) {
            if let Ok(pid) = i32::try_from(pid) {
                // SAFETY: callers provide only recorded fixture pids and their descendants.
                unsafe {
                    libc::kill(pid, libc::SIGCONT);
                    libc::kill(pid, signal);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while pids.iter().any(|p| running(*p)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
pub fn children(root: &Path, children: &[u32]) {
    let roots = children.iter().copied().collect();
    let mut ps = isolated("/bin/ps", root);
    let owned = process::try_output(ps.args(["-A", "-o", "pid=,ppid=,comm="]))
        .ok()
        .map(|output| {
            watchdog::descendants(
                &roots,
                &tmux_sidecar::proc::parse(&String::from_utf8_lossy(&output.stdout)),
            )
        })
        .unwrap_or(roots);
    terminate(&owned.into_iter().collect::<Vec<_>>());
}
pub fn fixture(root: &Path, server: Option<u32>, diagnose: bool) {
    let daemon = std::fs::read_to_string(root.join("run/daemon.pid"))
        .ok()
        .and_then(|s| s.parse().ok());
    let roots: BTreeSet<_> = server
        .into_iter()
        .chain(daemon)
        .filter(|p| *p > 1)
        .collect();
    let mut ps = isolated("/bin/ps", root);
    let owned = process::try_output(ps.args(["-A", "-o", "pid=,ppid=,comm="]))
        .ok()
        .map(|output| {
            let snapshot = tmux_sidecar::proc::parse(&String::from_utf8_lossy(&output.stdout));
            let owned = watchdog::descendants(&roots, &snapshot);
            if diagnose {
                for pid in &owned {
                    super::stage(&format!("scratch ps: pid={pid}"));
                }
            }
            owned
        })
        .unwrap_or(roots);
    let mut stop = isolated("tmux", root);
    if let Ok(socket) = std::fs::read_to_string(root.join("tmux.socket")) {
        stop.args(["-S", socket.trim()]);
    } else if let Some(name) = root.file_name() {
        stop.arg("-L").arg(name);
    }
    let _ = process::try_output(stop.arg("kill-server"));
    // A daemon needs the original server key to accept stop; use the recorded socket when available.
    if let Ok(socket) = std::fs::read_to_string(root.join("tmux.socket")) {
        let _ = process::try_output(isolated(env!("CARGO_BIN_EXE_tmux-sidecar"), root).args([
            "--tmux-socket",
            socket.trim(),
            "daemon",
            "stop",
        ]));
    }
    terminate(&owned.into_iter().collect::<Vec<_>>());
}

pub fn remove_tree(root: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !root.exists() {
        return Ok(());
    }
    fn repair(root: &Path) -> std::io::Result<()> {
        let meta = std::fs::symlink_metadata(root)?;
        if !meta.is_dir() {
            return Ok(());
        }
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        for entry in std::fs::read_dir(root)? {
            repair(&entry?.path())?;
        }
        Ok(())
    }
    repair(root)?;
    std::fs::remove_dir_all(root)
}
