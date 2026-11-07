use super::{Check, agents};
use crate::model::AgentKind;
use std::{fs, io, os::unix::fs::MetadataExt, path::Path, time::Duration};

pub(super) fn tmux_version(text: &str) -> io::Result<String> {
    let version = text.trim().strip_prefix("tmux ").unwrap_or(text.trim());
    if version == "master" {
        return Ok("tmux master: unknown build, assumed current".into());
    }
    // OpenBSD's base-system releases include tmux >=3.3 starting with 7.3.
    let (version, minimum) = if let Some(v) = version.strip_prefix("openbsd-") {
        (v, (7, 3))
    } else {
        (version.strip_prefix("next-").unwrap_or(version), (3, 3))
    };
    let mut parts = version.split('.');
    let major = parts.next().and_then(|s| s.parse::<u32>().ok());
    let minor = parts
        .next()
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse::<u32>().ok());
    if major.zip(minor).is_some_and(|v| v >= minimum) {
        Ok(text.trim().into())
    } else {
        Err(io::Error::other(format!(
            "unsupported tmux version: {}",
            text.trim()
        )))
    }
}
pub(super) fn permissions(path: &Path, uid: u32) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(format!("{} is a symlink", path.display())));
    }
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o7777 != 0o700 {
        return Err(io::Error::other(format!(
            "{} must be owned by uid {uid} with mode 0700",
            path.display()
        )));
    }
    Ok(format!("{} mode 0700, uid {uid}", path.display()))
}
pub(super) fn readable(path: &Path) -> io::Result<String> {
    let meta = fs::metadata(path)?;
    // Check mode bits as well as opening: tests may run with elevated privileges.
    if meta.mode() & 0o444 == 0 || (meta.is_dir() && meta.mode() & 0o111 == 0) {
        return Err(io::Error::other(format!(
            "{} is unreadable",
            path.display()
        )));
    }
    if !meta.is_dir() && !meta.is_file() {
        return Err(io::Error::other("store is not a file or directory"));
    }
    fs::File::open(path)?;
    Ok(format!("{} readable", path.display()))
}
pub(super) fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
}
pub(super) async fn local(home: &Path, binary: &Path, requested: &[AgentKind]) -> Vec<Check> {
    let mut command = crate::tmux::RealTmux {
        socket: crate::tmux::Socket::Name("unused".into()),
    }
    .command();
    command.arg("-V");
    let version = tokio::time::timeout(
        probe_timeout(),
        tokio::process::Command::from(command)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "tmux version probe timed out"))
    .and_then(|result| result)
    .map_err(|e| io::Error::new(e.kind(), format!("could not run tmux version probe: {e}")))
    .and_then(|output| {
        if output.status.success() {
            tmux_version(&String::from_utf8_lossy(&output.stdout))
        } else {
            Err(io::Error::other(format!(
                "tmux version probe failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    });
    let slow = version
        .as_ref()
        .is_err_and(|e| e.kind() == io::ErrorKind::TimedOut);
    let (version, fix) = if slow {
        (
            Err(io::Error::other("tmux slow to answer")),
            "retry when the machine is less busy; check tmux responsiveness",
        )
    } else {
        (version, "install tmux >= 3.3")
    };
    let mut checks = vec![Check::new("tmux", version, fix)];
    let notifier = if cfg!(target_os = "macos") {
        "osascript"
    } else {
        "notify-send"
    };
    let available = std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|p| executable(&p.join(notifier))));
    checks.push(Check::new(
        "notifier",
        if available {
            Ok(notifier.into())
        } else {
            Err(io::Error::other(format!("{notifier} missing from PATH")))
        },
        "install libnotify (Linux) or restore /usr/bin/osascript (macOS)",
    ));
    let (claude, codex) = session_board::source::homes(home);
    let (grok, opencode) = session_board::source::database_paths(home);
    checks.extend(agents::checks(
        home,
        binary,
        requested,
        &[
            (AgentKind::Claude, claude.join("projects")),
            (AgentKind::Codex, codex.join("sessions")),
            (AgentKind::Grok, grok),
            (AgentKind::Opencode, opencode),
        ],
    ));
    checks
}

// access checks the current uid, ACLs and read-only mounts without modifying history.
pub(super) fn writable(path: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let meta =
        fs::metadata(path).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    let name = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: name is a live NUL-terminated path; access does not retain it.
    let allowed = unsafe { libc::access(name.as_ptr(), libc::W_OK) } == 0;
    if !allowed || meta.mode() & 0o222 == 0 {
        return Err(io::Error::other(format!(
            "{} is not writable; check write permissions",
            path.display()
        )));
    }
    Ok(())
}

fn probe_timeout() -> Duration {
    #[cfg(feature = "test-util")]
    let multiplier = std::env::var("TMUX_SIDECAR_TEST_TIMEOUT_MULTIPLIER")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1)
        .clamp(1, 4);
    #[cfg(not(feature = "test-util"))]
    let multiplier = 1;
    Duration::from_secs(1) * multiplier
}
