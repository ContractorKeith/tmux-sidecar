use std::{io, path::Path};
pub fn shared(home: &Path) -> io::Result<bool> {
    let path = home.join(".grok/config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
    };
    let config: toml::Value =
        toml::from_str(&text).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    Ok(config
        .get("claude_hooks_enabled")
        .and_then(toml::Value::as_bool)
        != Some(false)
        && config
            .get("compat")
            .and_then(|v| v.get("claude"))
            .and_then(|v| v.get("hooks"))
            .and_then(toml::Value::as_bool)
            != Some(false))
}
