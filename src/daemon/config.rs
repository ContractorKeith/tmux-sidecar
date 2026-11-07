use super::*;
fn error(path: &std::path::Path, message: impl std::fmt::Display) -> io::Error {
    let reason = message
        .to_string()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    io::Error::other(format!("config {}: {reason}", path.display()))
}
/// Shared validation keeps every duration safe before constructing timers.
pub fn validate(path: &std::path::Path) -> io::Result<toml::Table> {
    let table = match fs::read_to_string(path) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(|e| error(path, e))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(error(path, e)),
    };
    for (key, min, max) in [
        ("poll_ms", 50, 60000),
        ("idle_after_secs", 0, 86400),
        ("send_confirm_secs", 0, 86400),
    ] {
        bounded(path, key, table.get(key), min, max)?;
    }
    bounded(
        path,
        "notify.debounce_secs",
        table.get("notify").and_then(|v| v.get("debounce_secs")),
        0,
        3600,
    )?;
    Ok(table)
}
fn bounded(
    path: &std::path::Path,
    key: &str,
    value: Option<&toml::Value>,
    min: u64,
    max: u64,
) -> io::Result<Option<u64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .as_integer()
        .and_then(|v| u64::try_from(v).ok())
        .filter(|v| (min..=max).contains(v))
        .map(Some)
        .ok_or_else(|| error(path, format!("{key} must be an integer in {min}..={max}")))
}
pub fn poll_ms(paths: &Paths) -> io::Result<u64> {
    let table = validate(&paths.config)?;
    let override_value = std::env::var_os("TMUX_SIDECAR_POLL_MS");
    let value = if let Some(value) = &override_value {
        let value = value.to_str().ok_or_else(|| {
            io::Error::other("TMUX_SIDECAR_POLL_MS (poll_ms) must be a UTF-8 integer")
        })?;
        value
            .parse::<u64>()
            .map_err(|e| io::Error::other(format!("TMUX_SIDECAR_POLL_MS (poll_ms): {e}")))?
    } else {
        table
            .get("poll_ms")
            .and_then(|v| v.as_integer())
            .unwrap_or(1000) as u64
    };
    if !(50..=60000).contains(&value) {
        return Err(if override_value.is_some() {
            io::Error::other("TMUX_SIDECAR_POLL_MS (poll_ms) must be an integer in 50..=60000")
        } else {
            error(&paths.config, "poll_ms must be an integer in 50..=60000")
        });
    }
    Ok(value)
}
pub fn idle_after_ms(paths: &Paths) -> io::Result<u64> {
    let table = validate(&paths.config)?;
    Ok(table
        .get("idle_after_secs")
        .and_then(|v| v.as_integer())
        .unwrap_or(1800) as u64
        * 1000)
}
pub fn send_confirm_ms(paths: &Paths) -> io::Result<u64> {
    let table = validate(&paths.config)?;
    Ok(table
        .get("send_confirm_secs")
        .and_then(|v| v.as_integer())
        .unwrap_or(30) as u64
        * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_rejects_out_of_range_durations_with_key_and_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths {
            runtime: root.path().into(),
            state: root.path().into(),
            config: root.path().join("config.toml"),
        };
        for (key, value) in [
            ("poll_ms", 0),
            ("poll_ms", 49),
            ("poll_ms", 60001),
            ("idle_after_secs", 86401),
            ("send_confirm_secs", 86401),
            ("notify.debounce_secs", 3601),
        ] {
            let text = if key.starts_with("notify.") {
                format!("[notify]\ndebounce_secs={value}")
            } else {
                format!("{key}={value}")
            };
            fs::write(&paths.config, text).unwrap();
            let result = match key {
                "idle_after_secs" => idle_after_ms(&paths),
                "send_confirm_secs" => send_confirm_ms(&paths),
                _ => validate(&paths.config).map(|_| 0),
            };
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains(key) && error.contains(paths.config.to_str().unwrap()),
                "{error}"
            );
        }
    }
}
