//! Resolve overrides for the current home; explicit alternate homes stay isolated.
use std::path::{Path, PathBuf};
pub fn config(home: &Path, relative: &str) -> PathBuf {
    if std::env::var_os("HOME").as_deref() == Some(home.as_os_str()) {
        for (prefix, variable) in [
            (".claude/", "CLAUDE_CONFIG_DIR"),
            (".codex/", "CODEX_HOME"),
            (".config/", "XDG_CONFIG_HOME"),
        ] {
            if let Some(rest) = relative.strip_prefix(prefix)
                && let Some(dir) = std::env::var_os(variable).filter(|v| !v.is_empty())
            {
                return PathBuf::from(dir).join(rest);
            }
        }
    }
    home.join(relative)
}
