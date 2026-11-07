//! Local diagnostics never auto-start or modify the daemon or agent stores.
#[cfg(test)]
mod agent_tests;
mod agents;
mod health;
mod hooks;
mod inspect;
#[cfg(test)]
mod tests;
use crate::{client, model::AgentKind, paths};
use serde::Serialize;
use std::{io, path::Path};

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Fail,
    Skip,
}

#[derive(Debug, Serialize)]
struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
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
            status: if ok { Status::Ok } else { Status::Fail },
            detail,
            fix: if ok { String::new() } else { fix.into() },
        }
    }
    fn skip(name: &str, detail: &str) -> Self {
        Self {
            name: name.into(),
            status: Status::Skip,
            detail: detail.into(),
            fix: String::new(),
        }
    }
    fn human(&self) -> String {
        let status = match self.status {
            Status::Ok => "ok",
            Status::Fail => "fail",
            Status::Skip => "skip",
        };
        let mut line = format!("{status} {}: {}", self.name, self.detail);
        if self.status == Status::Fail {
            line.push_str(&format!("; fix: {}", self.fix));
        }
        // Keep each diagnostic on one line without quoting readable paths.
        line.replace(['\n', '\r'], " ")
    }
}
fn daemon_check(result: io::Result<serde_json::Value>) -> Check {
    if result.as_ref().is_err_and(|e| {
        matches!(
            e.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
        )
    }) {
        return Check::skip("daemon", "not running; started on first use");
    }
    Check::new(
        "daemon",
        result.and_then(|value| {
            let version = value["daemon_version"].as_str().unwrap_or("unknown");
            if version != env!("CARGO_PKG_VERSION")
                || value["build_id"].as_str() != Some(env!("TMUX_SIDECAR_BUILD_ID"))
            {
                return Err(io::Error::other(format!(
                    "daemon {version} ({}), client {} ({})",
                    value["build_id"].as_str().unwrap_or("unknown build"),
                    env!("CARGO_PKG_VERSION"),
                    env!("TMUX_SIDECAR_BUILD_ID")
                )));
            }
            Ok(format!("reachable, version {version}"))
        }),
        "resolve any config/database failures below; then run `tmux-sidecar list` (inside tmux); if reachable, run `tmux-sidecar daemon stop` to update its binary",
    )
}
pub async fn run(
    explicit: Option<&Path>,
    json: bool,
    quiet: bool,
    requested: &[AgentKind],
) -> Result<(), (u8, String)> {
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
    let mut checks = inspect::local(&home, &binary, requested).await;
    let hello = match (&socket, &dirs) {
        (Ok(socket), Some(dirs)) => {
            client::request(socket, dirs, "hello", serde_json::json!({}), false).await
        }
        _ => Err(io::Error::other(
            "cannot select tmux server; run inside tmux or use --tmux-socket",
        )),
    };
    checks.push(daemon_check(hello));
    if let Some(dirs) = &dirs {
        checks.extend(health::checks(dirs));
    }
    for (name, path) in [
        ("runtime permissions", dirs.as_ref().map(|p| &p.runtime)),
        ("state permissions", dirs.as_ref().map(|p| &p.state)),
    ] {
        if let Some(path) = path
            && std::fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
        {
            checks.push(Check::skip(
                name,
                &format!("{} absent; created on first use", path.display()),
            ));
            continue;
        }
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
        use std::io::Write;
        let mut out = io::stdout().lock();
        if json {
            writeln!(
                out,
                "{}",
                serde_json::to_string(&checks).map_err(|e| (1, e.to_string()))?
            )
            .map_err(crate::cli::stdout_error)?;
        } else {
            for check in &checks {
                writeln!(out, "{}", check.human()).map_err(crate::cli::stdout_error)?;
            }
        }
    }
    if checks.iter().all(|c| c.status != Status::Fail) {
        Ok(())
    } else {
        Err((1, "doctor found missing or drifted checks".into()))
    }
}
