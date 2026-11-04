//! M0: process snapshot adapter for Linux and macOS.
use crate::seams::{Process, ProcessSnapshot, ProcessTree};
use std::{io, path::Path, process::Command};

pub struct PsProcessTree;
impl ProcessTree for PsProcessTree {
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
