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
    for key in ["poll_ms", "idle_after_secs", "send_confirm_secs"] {
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
    if let Some(parent) = path.parent().filter(|p| p.exists()) {
        super::inspect::writable(parent)?;
    }
    for file in [
        path.to_owned(),
        path.with_file_name("tmux-sidecar.db-shm"),
        path.with_file_name("tmux-sidecar.db-wal"),
    ] {
        if file.exists() {
            super::inspect::writable(&file)?;
        }
    }
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
    result.map_err(|e| {
        if matches!(&e, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::NotADatabase) {
            named(path, "not a tmux-sidecar database; move it aside")
        } else { named(path, e) }
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn database_requires_writable_database_sidecars_and_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tmux-sidecar.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE meta(schema_version); INSERT INTO meta VALUES(3);")
            .unwrap();
        drop(db);
        for target in [
            path.clone(),
            root.path().join("tmux-sidecar.db-shm"),
            root.path().join("tmux-sidecar.db-wal"),
            root.path().to_owned(),
        ] {
            if target != path && target != root.path() {
                fs::write(&target, "").unwrap();
            }
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if target.is_dir() { 0o500 } else { 0o400 }),
            )
            .unwrap();
            let result = database(&path);
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if target.is_dir() { 0o700 } else { 0o600 }),
            )
            .unwrap();
            assert!(result.is_err(), "{} passed", target.display());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains(target.to_str().unwrap())
            );
            if target != path && target != root.path() {
                fs::remove_file(target).unwrap();
            }
        }
    }
    #[test]
    fn non_sqlite_file_has_actionable_hint() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("tmux-sidecar.db");
        fs::write(&path, "garbage").unwrap();
        assert!(
            database(&path)
                .unwrap_err()
                .to_string()
                .contains("not a tmux-sidecar database; move it aside")
        );
    }
}
