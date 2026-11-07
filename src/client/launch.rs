//! Shared detached-daemon launch policy for clients and hooks.
use std::{ffi::OsStr, io, os::unix::process::CommandExt, process::Command};
fn allowed(key: &OsStr) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    matches!(
        key,
        "HOME"
            | "PATH"
            | "LANG"
            | "TMPDIR"
            | "CLAUDE_CONFIG_DIR"
            | "CODEX_HOME"
            | "GROK_HOME"
            | "OPENCODE_DB"
    ) || ["LC_", "XDG_", "TMUX_SIDECAR_"]
        .iter()
        .any(|prefix| key.starts_with(prefix))
}
pub(super) fn prepare(command: &mut Command) -> io::Result<()> {
    command
        .env_clear()
        .envs(std::env::vars_os().filter(|(key, _)| allowed(key)));
    #[cfg(target_os = "macos")]
    let max_fd = {
        let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
        // SAFETY: limit is writable storage for exactly one rlimit.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: getrlimit succeeded and initialized limit.
        unsafe { limit.assume_init() }.rlim_cur.min(i32::MAX as u64) as i32
    };
    // SAFETY: only async-signal-safe syscalls run after fork. Mark all fd >= 3
    // CLOEXEC, including concurrent pipe creations; keep Rust's error pipe until exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(target_os = "linux")]
            if libc::close_range(3, u32::MAX, libc::CLOSE_RANGE_CLOEXEC as i32) != 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(target_os = "macos")]
            for fd in 3..max_fd {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
            Ok(())
        });
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn whitelist_has_only_daemon_inputs() {
        for name in [
            "HOME",
            "PATH",
            "LC_ALL",
            "XDG_DATA_HOME",
            "TMUX_SIDECAR_POLL_MS",
            "CODEX_HOME",
        ] {
            assert!(super::allowed(name.as_ref()));
        }
        for name in ["CLAUDECODE", "TMUX_PANE", "TMUX", "API_KEY", "SHELL"] {
            assert!(!super::allowed(name.as_ref()));
        }
    }
}
