use std::{
    fs, io,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
pub fn target(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_owned();
    for _ in 0..40 {
        match fs::read_link(&path) {
            Ok(link) => {
                path = if link.is_absolute() {
                    link
                } else {
                    path.parent()
                        .ok_or_else(|| io::Error::other("missing parent"))?
                        .join(link)
                };
            }
            Err(e)
                if e.kind() == io::ErrorKind::InvalidInput
                    || e.kind() == io::ErrorKind::NotFound =>
            {
                return if path.exists() {
                    path.canonicalize()
                } else {
                    Ok(path)
                };
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many config symlinks"))
}
pub fn read(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}
pub fn backup(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmux-sidecar.bak");
    name.into()
}
fn normalize(value: &mut serde_json::Value) {
    if let Some(object) = value.as_object_mut()
        && let Some(hooks) = object
            .get_mut("hooks")
            .and_then(serde_json::Value::as_object_mut)
    {
        hooks.retain(|_, v| !v.as_array().is_some_and(Vec::is_empty));
        if hooks.is_empty() {
            object.remove("hooks");
        }
    }
}
pub fn equivalent(path: &Path, a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Option<serde_json::Value> {
        if path.extension().is_some_and(|s| s == "json") {
            if s.trim().is_empty() {
                Some(serde_json::json!({}))
            } else {
                serde_json::from_str(s).ok()
            }
        } else if path.extension().is_some_and(|s| s == "toml") {
            serde_json::to_value(toml::from_str::<toml::Value>(s).ok()?).ok()
        } else {
            Some(serde_json::Value::String(s.to_owned()))
        }
    };
    parse(a).zip(parse(b)).is_some_and(|(mut a, mut b)| {
        normalize(&mut a);
        normalize(&mut b);
        a == b
    })
}
pub fn apply(
    path: &Path,
    before: &str,
    after: Option<&str>,
    dry_run: bool,
    install: bool,
) -> io::Result<()> {
    let after_text = after.unwrap_or("");
    if before == after_text && (after.is_some() || !path.exists()) {
        if !install && !dry_run {
            remove_backup(path)?;
        }
        return Ok(());
    }
    if dry_run {
        use std::io::Write;
        io::stdout()
            .lock()
            .write_all(super::diff::unified(path, before, after_text).as_bytes())?;
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    if install && path.exists() && !backup(path).exists() {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(backup(path))?;
        file.write_all(before.as_bytes())?;
        file.sync_all()?;
    }
    let result = if let Some(after) = after {
        let temp = parent.join(format!(".tmux-sidecar-{}.tmp", std::process::id()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(after.as_bytes())?;
            if let Ok(meta) = fs::metadata(path) {
                file.set_permissions(fs::Permissions::from_mode(meta.permissions().mode()))?;
            }
            file.sync_all()?;
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    } else {
        fs::remove_file(path)
    };
    result?;
    if !install {
        remove_backup(path)?;
    }
    Ok(())
}
fn remove_backup(path: &Path) -> io::Result<()> {
    match fs::remove_file(backup(path)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
