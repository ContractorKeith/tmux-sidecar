//! Local diagnostics never auto-start or modify the daemon or agent stores.
mod hooks;
mod inspect;
#[cfg(test)]
mod tests;
use crate::{client, paths};
use serde::Serialize;
use std::{io, path::Path};

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub fix: String,
}
impl Check {
    fn new(name: &str, result: io::Result<String>, fix: &str) -> Self {
        let (ok, detail) = match result {
            Ok(detail) => (true, detail),
            Err(error) => (false, error.to_string()),
        };
        Self {
            name: name.into(),
            ok,
            detail,
            fix: fix.into(),
        }
    }
}
fn daemon_check(result: io::Result<serde_json::Value>) -> Check {
    Check::new(
        "daemon",
        result.and_then(|value| {
            let version = value["daemon_version"].as_str().unwrap_or("unknown");
            if version != env!("CARGO_PKG_VERSION") {
                return Err(io::Error::other(format!(
                    "daemon {version}, client {}",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            Ok(format!("reachable, version {version}"))
        }),
        "run `tmux-sidecar daemon stop`, then `tmux-sidecar list` (inside tmux)",
    )
}
pub async fn run(explicit: Option<&Path>, json: bool, quiet: bool) -> Result<(), (u8, String)> {
    let home = std::env::var_os("HOME").ok_or((1, "HOME is missing".into()))?;
    let home = std::path::PathBuf::from(home);
    let binary = std::env::current_exe().map_err(|e| (1, e.to_string()))?;
    let socket = client::select_socket(explicit);
    // Resolve without the normal runtime security rejection so doctor can report it.
    let key = socket.as_ref().ok().and_then(|s| paths::server_key(s).ok());
    let dirs = key.as_ref().map(|key| {
        paths::resolve(
            if cfg!(target_os = "macos") {
                paths::Platform::Macos
            } else {
                paths::Platform::Linux
            },
            &home,
            paths::uid(),
            key,
            |name| std::env::var_os(name),
        )
    });
    let mut checks = inspect::local(&home, &binary).await;
    let hello = match (&socket, &dirs) {
        (Ok(socket), Some(dirs)) => {
            client::request(socket, dirs, "hello", serde_json::json!({}), false).await
        }
        _ => Err(io::Error::other(
            "cannot select tmux server; run inside tmux or use --tmux-socket",
        )),
    };
    checks.push(daemon_check(hello));
    for (name, path) in [
        ("runtime permissions", dirs.as_ref().map(|p| &p.runtime)),
        ("state permissions", dirs.as_ref().map(|p| &p.state)),
    ] {
        checks.push(Check::new(
            name,
            path.map_or_else(
                || Err(io::Error::other("tmux server unavailable")),
                |path| inspect::permissions(path, paths::uid()),
            ),
            "create the directory owned by your uid with mode 0700; see detail for path",
        ));
    }
    if !quiet {
        if json {
            println!(
                "{}",
                serde_json::to_string(&checks).map_err(|e| (1, e.to_string()))?
            );
        } else {
            for check in &checks {
                println!(
                    "{} {}: {}; fix: {}",
                    if check.ok { "ok" } else { "missing" },
                    check.name,
                    check.detail.escape_debug(),
                    check.fix.escape_debug()
                );
            }
        }
    }
    if checks.iter().all(|c| c.ok) {
        Ok(())
    } else {
        Err((1, "doctor found missing or drifted checks".into()))
    }
}
