use std::{io, path::Path};
pub fn shared(home: &Path) -> io::Result<bool> {
    let text = match std::fs::read_to_string(home.join(".grok/config.toml")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    let config: toml::Value = toml::from_str(&text).map_err(io::Error::other)?;
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
