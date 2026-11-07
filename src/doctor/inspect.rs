use super::{Check, hooks};
use std::{fs, io, os::unix::fs::MetadataExt, path::Path, time::Duration};

pub(super) fn tmux_version(text: &str) -> io::Result<String> {
    let version = text.trim().strip_prefix("tmux ").unwrap_or(text.trim());
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
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o7777 != 0o700 {
        return Err(io::Error::other(format!(
            "{} must be owned by uid {uid} with mode 0700",
            path.display()
        )));
    }
    Ok(format!("{} mode 0700, uid {uid}", path.display()))
}
pub(super) fn readable(path: &Path) -> io::Result<String> {
    let meta = fs::symlink_metadata(path)?;
    // Check mode bits as well as opening: tests may run with elevated privileges.
    if meta.mode() & 0o444 == 0 || (meta.is_dir() && meta.mode() & 0o111 == 0) {
        return Err(io::Error::other(format!(
            "{} is unreadable",
            path.display()
        )));
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            readable(&entry.path())?;
        }
    } else if meta.is_file() {
        fs::File::open(path)?;
    } else {
        return Err(io::Error::other("store is not a file or directory"));
    }
    Ok(format!("{} readable", path.display()))
}
pub(super) fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
}
pub(super) async fn local(home: &Path, binary: &Path) -> Vec<Check> {
    let mut command = crate::tmux::RealTmux {
        socket: crate::tmux::Socket::Name("unused".into()),
    }
    .command();
    command.arg("-V");
    let version = crate::proc::bounded::output(
        command,
        tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await
    .and_then(|o| tmux_version(&String::from_utf8_lossy(&o.stdout)));
    let mut checks = vec![Check::new("tmux", version, "install tmux >= 3.3")];
    for agent in ["claude", "codex", "grok", "opencode"] {
        checks.push(Check::new(&format!("hooks {agent}"), hooks::check(home, binary, agent),
            &format!("run `tmux-sidecar hooks install --agent {agent}`; if paths drifted, uninstall then reinstall; Codex may need --chain")));
    }
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
    for (agent, path) in [
        ("claude", claude.join("projects")),
        ("codex", codex.join("sessions")),
        ("grok", grok),
        ("opencode", opencode),
    ] {
        checks.push(Check::new(&format!("store {agent}"), readable(&path).map_err(|e| io::Error::other(format!("{}: {e}", path.display()))),
            "run the agent once to create its store; check read/search permissions and agent home overrides"));
    }
    checks
}
