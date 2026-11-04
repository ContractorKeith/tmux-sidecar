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
        parse(&String::from_utf8(output.stdout).map_err(io::Error::other)?)
    }
}

pub fn parse(text: &str) -> io::Result<ProcessSnapshot> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid ps record");
            let (pid, rest) = line
                .trim_start()
                .split_once(char::is_whitespace)
                .ok_or_else(invalid)?;
            let (ppid, comm) = rest
                .trim_start()
                .split_once(char::is_whitespace)
                .ok_or_else(invalid)?;
            let comm = Path::new(comm.trim())
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(invalid)?;
            Ok((
                pid.parse().map_err(|_| invalid())?,
                Process {
                    ppid: ppid.parse().map_err(|_| invalid())?,
                    comm: comm.into(),
                },
            ))
        })
        .collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn linux_and_macos_records() {
        for text in [
            "  1 0 systemd\n 42 1 codex\n",
            " 1 0 /sbin/launchd\n 42 1 /Users/test name/bin/codex\n",
        ] {
            let snapshot = super::parse(text).unwrap();
            assert_eq!(snapshot[&42].comm, "codex");
            assert_eq!(snapshot[&42].ppid, 1);
        }
        assert!(super::parse("bad record").is_err());
    }
}
