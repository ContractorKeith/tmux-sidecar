//! Read-only startup diagnostics: never create or migrate the history database.
use super::Check;
use crate::paths::Paths;
use std::{fs, io, path::Path};
fn named(path: &Path, error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("{}: {}", path.display(), error.to_string().trim()))
}
fn config(path: &Path) -> io::Result<String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(format!("{} absent; defaults", path.display()));
        }
        Err(e) => return Err(named(path, e)),
    };
    let table: toml::Table = toml::from_str(&text).map_err(|e| named(path, e))?;
    for key in ["poll_ms", "idle_after_secs"] {
        if let Some(value) = table.get(key) {
            let valid = value.as_integer().is_some_and(|v| {
                if key == "poll_ms" {
                    v > 0
                } else {
                    v >= 0 && (v as u64).checked_mul(1000).is_some()
                }
            });
            if !valid {
                return Err(named(
                    path,
                    format!(
                        "{key} must be a valid nonnegative duration (poll_ms must be positive)"
                    ),
                ));
            }
        }
    }
    crate::notify::Config::read(path).map_err(|e| named(path, e))?;
    Ok(format!("{} parses", path.display()))
}
fn database(path: &Path) -> io::Result<String> {
    if !path.exists() {
        return Ok(format!(
            "{} absent; created on daemon startup",
            path.display()
        ));
    }
    let result = (|| -> rusqlite::Result<()> {
        let db = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.busy_timeout(std::time::Duration::from_millis(250))?;
        let status: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        if status != "ok" {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                io::Error::other(status),
            )));
        }
        let version: u32 = db.query_row("SELECT schema_version FROM meta", [], |r| r.get(0))?;
        if version > 3 {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                io::Error::other(format!(
                    "database schema {version} is newer than this binary (3); upgrade tmux-sidecar"
                )),
            )));
        }
        Ok(())
    })();
    result.map_err(|e| named(path, e))?;
    Ok(format!("{} opens read-only", path.display()))
}
pub(super) fn checks(paths: &Paths) -> Vec<Check> {
    vec![
        Check::new(
            "config",
            config(&paths.config),
            "correct the named config file",
        ),
        Check::new(
            "database",
            database(&paths.state.join("tmux-sidecar.db")),
            "upgrade for a newer schema; otherwise restore the named database from a backup",
        ),
    ]
}
