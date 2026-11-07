use super::*;
pub fn poll_ms(paths: &Paths) -> io::Result<u64> {
    let config = match fs::read_to_string(&paths.config) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };
    let value = if let Ok(value) = std::env::var("TMUX_SIDECAR_POLL_MS") {
        value.parse::<u64>().map_err(other)?
    } else if let Some(value) = config.get("poll_ms") {
        value
            .as_integer()
            .and_then(|n| n.try_into().ok())
            .ok_or_else(|| io::Error::other("poll_ms must be a positive integer"))?
    } else {
        1000
    };
    if value == 0 {
        return Err(io::Error::other("poll_ms must be positive"));
    }
    Ok(value)
}

pub fn idle_after_ms(paths: &Paths) -> io::Result<u64> {
    let table = match fs::read_to_string(&paths.config) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };
    let secs = match table.get("idle_after_secs") {
        None => 1800,
        Some(v) => v
            .as_integer()
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| io::Error::other("idle_after_secs must be nonnegative"))?,
    };
    secs.checked_mul(1000)
        .ok_or_else(|| io::Error::other("idle_after_secs overflows milliseconds"))
}

pub fn send_confirm_ms(paths: &Paths) -> io::Result<u64> {
    let table = match fs::read_to_string(&paths.config) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e),
    };
    let secs = match table.get("send_confirm_secs") {
        None => 30,
        Some(v) => v
            .as_integer()
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| io::Error::other("send_confirm_secs must be nonnegative"))?,
    };
    secs.checked_mul(1000)
        .ok_or_else(|| io::Error::other("send_confirm_secs overflows milliseconds"))
}
