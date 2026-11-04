//! Pure path resolution; callers create directories with mode 0700 in M1.
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy)]
pub enum Platform {
    Linux,
    Macos,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Paths {
    pub runtime: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
}

/// Absolute socket path, resolving symlinks when the socket exists.
/// A not-yet-created socket keeps its absolute spelling until it can be resolved.
pub fn canonical_socket(socket: &Path) -> io::Result<PathBuf> {
    socket
        .canonicalize()
        .or_else(|_| std::path::absolute(socket))
}

/// FNV-1a 64-bit over the canonical socket path bytes, truncated to 12 hex digits.
/// Relative paths and existing symlinks identify the same server as its absolute path.
pub fn server_key(socket: &Path) -> io::Result<String> {
    use std::os::unix::ffi::OsStrExt;
    let socket = canonical_socket(socket)?;
    let hash = socket
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    Ok(format!("{:012x}", hash & 0xffffffffffff))
}

/// Overrides name the complete per-server directory, without appending a key.
/// Linux without XDG_RUNTIME_DIR falls back to /tmp/tmux-sidecar-<uid>/<key>.
pub fn resolve(
    platform: Platform,
    home: &Path,
    uid: u32,
    key: &str,
    env: impl Fn(&str) -> Option<OsString>,
) -> Paths {
    let get = |name: &str| env(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let (runtime, state, config) = match platform {
        Platform::Linux => (
            get("XDG_RUNTIME_DIR")
                .map(|dir| dir.join("tmux-sidecar"))
                .unwrap_or_else(|| PathBuf::from(format!("/tmp/tmux-sidecar-{uid}")))
                .join(key),
            get("XDG_STATE_HOME")
                .unwrap_or_else(|| home.join(".local/state"))
                .join("tmux-sidecar")
                .join(key),
            get("XDG_CONFIG_HOME")
                .unwrap_or_else(|| home.join(".config"))
                .join("tmux-sidecar/config.toml"),
        ),
        Platform::Macos => (
            get("TMPDIR")
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(format!("tmux-sidecar-{uid}"))
                .join(key),
            home.join("Library/Application Support/tmux-sidecar")
                .join(key),
            home.join("Library/Application Support/tmux-sidecar/config.toml"),
        ),
    };
    Paths {
        runtime: get("TMUX_SIDECAR_RUNTIME_DIR").unwrap_or(runtime),
        state: get("TMUX_SIDECAR_STATE_DIR").unwrap_or(state),
        config,
    }
}

/// Extract the first comma-delimited field of TMUX, preserving non-UTF-8 path bytes.
pub fn socket_from_tmux(tmux: &OsStr) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = tmux.as_bytes().split(|b| *b == b',').next()?;
    (!bytes.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vars(name: &str) -> Option<OsString> {
        match name {
            "XDG_RUNTIME_DIR" => Some("/run/user/42".into()),
            "XDG_STATE_HOME" => Some("/state".into()),
            "XDG_CONFIG_HOME" => Some("/config".into()),
            "TMPDIR" => Some("/private/tmp".into()),
            _ => None,
        }
    }
    #[test]
    fn linux_paths() {
        assert_eq!(
            resolve(Platform::Linux, Path::new("/home/test"), 42, "key", vars),
            Paths {
                runtime: "/run/user/42/tmux-sidecar/key".into(),
                state: "/state/tmux-sidecar/key".into(),
                config: "/config/tmux-sidecar/config.toml".into()
            }
        );
        let paths = resolve(Platform::Linux, Path::new("/home/test"), 42, "key", |_| {
            None
        });
        assert_eq!(paths.runtime, Path::new("/tmp/tmux-sidecar-42/key"));
        assert_eq!(
            paths.state,
            Path::new("/home/test/.local/state/tmux-sidecar/key")
        );
        assert_eq!(
            paths.config,
            Path::new("/home/test/.config/tmux-sidecar/config.toml")
        );
    }
    #[test]
    fn macos_paths() {
        assert_eq!(
            resolve(Platform::Macos, Path::new("/Users/test"), 42, "key", vars),
            Paths {
                runtime: "/private/tmp/tmux-sidecar-42/key".into(),
                state: "/Users/test/Library/Application Support/tmux-sidecar/key".into(),
                config: "/Users/test/Library/Application Support/tmux-sidecar/config.toml".into()
            }
        );
    }
    #[test]
    fn overrides_on_both_platforms() {
        for platform in [Platform::Linux, Platform::Macos] {
            let paths = resolve(
                platform,
                Path::new("/home/test"),
                42,
                "key",
                |name| match name {
                    "TMUX_SIDECAR_RUNTIME_DIR" => Some("/override/run".into()),
                    "TMUX_SIDECAR_STATE_DIR" => Some("/override/state".into()),
                    _ => None,
                },
            );
            assert_eq!(paths.runtime, Path::new("/override/run"));
            assert_eq!(paths.state, Path::new("/override/state"));
        }
    }
    #[test]
    fn socket_spellings_share_a_key() {
        use std::os::unix::{fs::symlink, net::UnixListener};
        let scratch = tempfile::tempdir().unwrap();
        let socket = scratch.path().join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        let alias = scratch.path().join("alias");
        symlink(&socket, &alias).unwrap();
        let cwd = std::env::current_dir().unwrap();
        let mut relative = PathBuf::new();
        for _ in cwd.components().skip(1) {
            relative.push("..");
        }
        relative.push(socket.strip_prefix("/").unwrap());
        assert_eq!(server_key(&relative).unwrap(), server_key(&socket).unwrap());
        assert_eq!(server_key(&alias).unwrap(), server_key(&socket).unwrap());
        let missing = scratch.path().join("missing");
        assert_eq!(canonical_socket(&missing).unwrap(), missing);
    }
    #[test]
    fn stable_keys_and_socket() {
        assert_eq!(server_key(Path::new("/hello")).unwrap(), "545fbff8a45e");
        assert_ne!(
            server_key(Path::new("/tmp/a")).unwrap(),
            server_key(Path::new("/tmp/b")).unwrap()
        );
        assert_eq!(
            socket_from_tmux(OsStr::new("/tmp/tmux-42/default,123,0")),
            Some("/tmp/tmux-42/default".into())
        );
        assert_eq!(socket_from_tmux(OsStr::new("")), None);
    }
}
