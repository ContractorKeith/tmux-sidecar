//! Fill every child environment explicitly; never inherit a developer's agent paths.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};
fn value(command: &Command, key: &str) -> Option<OsString> {
    command
        .get_envs()
        .find(|(name, _)| *name == key)
        .and_then(|(_, v)| v.map(OsString::from))
}
fn missing(command: &mut Command, key: &str, value: impl AsRef<std::ffi::OsStr>) {
    if !command.get_envs().any(|(name, _)| name == key) {
        command.env(key, value);
    }
}
pub fn fill(command: &mut Command) -> Option<tempfile::TempDir> {
    let scratch = value(command, "HOME")
        .is_none()
        .then(|| tempfile::tempdir().unwrap());
    let home = value(command, "HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.as_ref().unwrap().path().to_owned());
    apply(command, &home);
    scratch
}
pub fn apply(command: &mut Command, home: &Path) {
    // Rustup still needs the installed compiler while HOME is isolated.
    let original = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    for (key, fallback) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
        missing(
            command,
            key,
            std::env::var_os(key).unwrap_or_else(|| original.join(fallback).into()),
        );
    }
    missing(command, "HOME", home);
    for (key, relative) in [
        ("TMUX_TMPDIR", ""),
        ("TMUX_SIDECAR_RUNTIME_DIR", "run"),
        ("TMUX_SIDECAR_STATE_DIR", "state"),
        ("CLAUDE_CONFIG_DIR", ".claude"),
        ("CODEX_HOME", ".codex"),
        ("GROK_HOME", ".grok"),
        ("OPENCODE_DB", "opencode.db"),
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_STATE_HOME", ".local/state"),
        ("XDG_DATA_HOME", ".local/share"),
    ] {
        missing(command, key, home.join(relative));
    }
    for key in ["TMUX", "TMUX_PANE", "BASH_ENV", "ENV"] {
        if !command.get_envs().any(|(name, _)| name == key) {
            command.env_remove(key);
        }
    }
}
