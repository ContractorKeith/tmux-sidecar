//! Optional agents are skipped until a managed hook marker or explicit request exists.
use super::{Check, hooks, inspect};
use crate::model::AgentKind;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn installed(home: &Path, agent: AgentKind) -> io::Result<bool> {
    let files: &[&str] = match agent {
        AgentKind::Claude => &[".claude/settings.json"],
        AgentKind::Codex => &[".codex/config.toml"],
        AgentKind::Grok => &[
            ".grok/hooks/tmux-sidecar.json",
            ".grok/hooks/tmux-sidecar-common.json",
        ],
        AgentKind::Opencode => &[
            ".config/opencode/plugins/tmux-sidecar.js",
            ".config/opencode/plugin/tmux-sidecar.js",
        ],
    };
    for file in files {
        match fs::read_to_string(home.join(file)) {
            Ok(text)
                if text.contains("# tmux-sidecar")
                    || text.contains("\"_tmux_sidecar\"")
                    || text.contains("// tmux-sidecar: managed plugin") =>
            {
                return Ok(true);
            }
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}
pub(super) fn checks(
    home: &Path,
    binary: &Path,
    requested: &[AgentKind],
    stores: &[(AgentKind, PathBuf)],
) -> Vec<Check> {
    let mut checks = Vec::new();
    for (agent, store) in stores {
        let name = match agent {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Grok => "grok",
            AgentKind::Opencode => "opencode",
        };
        let hooked = installed(home, *agent);
        let hook_name = format!("hooks {name}");
        let check = match &hooked {
            Ok(false) if !requested.contains(agent) => {
                Check::skip(&hook_name, "no managed hooks installed")
            }
            _ => Check::new(
                &hook_name,
                hooked
                    .as_ref()
                    .map_err(|e| io::Error::other(e.to_string()))
                    .and_then(|_| hooks::check(home, binary, name)),
                &format!(
                    "run `tmux-sidecar hooks install --agent {name}`; uninstall then reinstall if paths drifted"
                ),
            ),
        };
        checks.push(check);
        let store_name = format!("store {name}");
        let result = inspect::readable(store);
        let absent = fs::symlink_metadata(store)
            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            && result
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound);
        checks.push(if absent && !hooked.unwrap_or(false) {
            Check::skip(&store_name, &format!("{} absent; agent has no managed hooks", store.display()))
        } else {
            Check::new(&store_name, result.map_err(|e| io::Error::other(format!("{}: {e}", store.display()))),
                "run the agent once to create its store; check read/search permissions and agent home overrides")
        });
    }
    checks
}
