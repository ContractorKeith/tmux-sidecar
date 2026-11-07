//! Default installation follows PATH; explicit agents and uninstall remain deterministic.
use super::{
    Hooks,
    execute::{Failure, output_error},
    stdout_error,
};
use crate::model::AgentKind;
use std::{io, io::Write, os::unix::fs::PermissionsExt, path::Path};
pub(super) fn run(command: &Hooks, quiet: bool) -> Result<(), Failure> {
    let (install, chain, agents, dry_run) = match command {
        Hooks::Install {
            chain,
            agent,
            dry_run,
        } => (true, *chain, agent, *dry_run),
        Hooks::Uninstall { agent, dry_run } => (false, false, agent, *dry_run),
    };
    let mut selected = agents.clone();
    if install && agents.is_empty() {
        for (name, agent) in [
            ("claude", AgentKind::Claude),
            ("codex", AgentKind::Codex),
            ("grok", AgentKind::Grok),
            ("opencode", AgentKind::Opencode),
        ] {
            let available = std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|dir| {
                    std::fs::metadata(dir.join(name))
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                })
            });
            if available {
                selected.push(agent);
            } else if !quiet {
                writeln!(io::stdout().lock(), "skip {name}: CLI not on PATH")
                    .map_err(stdout_error)?;
            }
        }
        if selected.is_empty() {
            return Ok(());
        }
    }
    let home = std::env::var_os("HOME").ok_or_else(|| (1, "HOME is missing".into()))?;
    let binary = std::env::current_exe().map_err(output_error)?;
    crate::install::run(
        Path::new(&home),
        &binary,
        &selected,
        install,
        chain,
        dry_run,
    )
    .map_err(output_error)
}
