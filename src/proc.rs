//! M0: process snapshot adapter for Linux and macOS.
mod ancestors;
pub(crate) mod bounded;
mod cwd;
mod liveness;
use crate::seams::{Process, ProcessSnapshot, ProcessTree};
pub use ancestors::hook_ancestors;
pub use liveness::running;
use std::{io, path::Path, process::Command};

pub fn agent_kind(comm: &str) -> Option<crate::model::AgentKind> {
    match comm {
        "claude" => Some(crate::model::AgentKind::Claude),
        "codex" => Some(crate::model::AgentKind::Codex),
        "grok" => Some(crate::model::AgentKind::Grok),
        "opencode" | "opencode.exe" => Some(crate::model::AgentKind::Opencode),
        _ => None,
    }
}
pub struct PsProcessTree;
impl ProcessTree for PsProcessTree {
    fn cwd(&self, pid: u32) -> io::Result<std::path::PathBuf> {
        cwd::read(pid)
    }
    fn snapshot(&self) -> io::Result<ProcessSnapshot> {
        let output = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,comm="])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Ok(parse(
            &String::from_utf8(output.stdout).map_err(io::Error::other)?,
        ))
    }
}

impl PsProcessTree {
    pub(crate) async fn snapshot_before(
        deadline: tokio::time::Instant,
    ) -> io::Result<ProcessSnapshot> {
        let mut command = Command::new("ps");
        command.args(["-A", "-o", "pid=,ppid=,comm="]);
        let output = bounded::output(command, deadline).await?;
        Ok(parse(
            &String::from_utf8(output.stdout).map_err(io::Error::other)?,
        ))
    }
}

/// Malformed process rows are transient; preserve the rest of the snapshot.
pub fn parse(text: &str) -> ProcessSnapshot {
    text.lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let (ppid, comm) = rest.trim_start().split_once(char::is_whitespace)?;
            let comm = comm.trim();
            if comm.is_empty() {
                return None;
            }
            let comm = if comm.starts_with('/') {
                Path::new(comm).file_name()?.to_str()?
            } else {
                comm
            };
            Some((
                pid.parse().ok()?,
                Process {
                    ppid: ppid.parse().ok()?,
                    comm: comm.into(),
                },
            ))
        })
        .collect()
}
pub fn alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 probes a positive pid without delivering a signal.
    unsafe {
        libc::kill(pid, 0) == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

pub fn terminate(pid: u32) {
    if pid == 0 {
        return;
    }
    if let Ok(pid) = i32::try_from(pid) {
        // SAFETY: positive pid is owned by the startup guard; SIGTERM permits cleanup.
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn malformed_rows_do_not_hide_agents_or_mangle_kernel_threads() {
        let snapshot = super::parse(
            " 2 1 kworker/R-netns\n 123 1\n 124 1   \n bad 1 broken\n 42 1 /usr/bin/codex\n",
        );
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[&2].comm, "kworker/R-netns");
        assert_eq!(snapshot[&42].comm, "codex");
    }
    #[test]
    fn linux_and_macos_records() {
        for text in [
            "  1 0 systemd\n 42 1 codex\n",
            " 1 0 /sbin/launchd\n 42 1 /Users/test name/bin/codex\n",
        ] {
            let snapshot = super::parse(text);
            assert_eq!(snapshot[&42].comm, "codex");
            assert_eq!(snapshot[&42].ppid, 1);
        }
        assert!(super::parse("bad record").is_empty());
    }
}
