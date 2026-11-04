//! Marked, reversible config edits, prepared fully before the first write.
pub mod claude;
pub mod codex;
mod codex_groups;
mod files;
pub mod grok;
pub mod opencode;
use crate::model::AgentKind;
use std::{
    io,
    path::{Path, PathBuf},
};
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub fn run(
    home: &Path,
    binary: &Path,
    agents: &[AgentKind],
    install: bool,
    chain: bool,
    dry_run: bool,
) -> io::Result<()> {
    let binary = binary
        .to_str()
        .ok_or_else(|| io::Error::other("binary path is not UTF-8"))?;
    let selected = |a| agents.is_empty() || agents.contains(&a);
    let shared = grok::shared(home)?;
    let mut plans: Vec<(PathBuf, String, Option<String>)> = Vec::new();
    let mut prepare =
        |relative: &str, editor: &dyn Fn(&str, bool) -> io::Result<String>| -> io::Result<()> {
            let path = files::target(&home.join(relative))?;
            let before = files::read(&path)?;
            if !install && !path.exists() {
                return Ok(());
            }
            let mut after = editor(&before, install)?;
            if files::equivalent(&path, &before, &after) {
                after = before.clone();
            }
            let backup = files::backup(&path);
            let original = files::read(&backup)?;
            // Restore bytes only when the current file is exactly our generated version.
            // Semantic equality alone would discard comments added after installation.
            let after = if !install && before == editor(&original, true)? {
                backup.exists().then_some(original)
            } else {
                Some(after)
            };
            plans.push((path, before, after));
            Ok(())
        };
    if selected(AgentKind::Claude) || (selected(AgentKind::Grok) && shared) {
        prepare(".claude/settings.json", &|s, mode| {
            claude::edit(s, mode, &format!("{} hook claude", quote(binary)))
        })?;
    }
    if selected(AgentKind::Grok) && !shared {
        prepare(".grok/hooks/tmux-sidecar.json", &|s, mode| {
            claude::edit(s, mode, &format!("{} hook grok", quote(binary)))
        })?;
    }
    if selected(AgentKind::Codex) {
        prepare(".codex/config.toml", &|s, mode| {
            codex::edit(s, mode, chain || !install, binary)
        })?;
    }
    if selected(AgentKind::Opencode) {
        prepare(".config/opencode/plugin/tmux-sidecar.js", &|s, mode| {
            if !s.is_empty() && !s.starts_with(opencode::MARKER) {
                return if mode {
                    Err(io::Error::other(
                        "refusing to overwrite unmarked OpenCode plugin",
                    ))
                } else {
                    Ok(s.to_owned())
                };
            }
            Ok(if mode {
                opencode::plugin(binary)
            } else {
                String::new()
            })
        })?;
    }
    for (path, before, after) in plans {
        files::apply(&path, &before, after.as_deref(), dry_run, install)?;
    }
    if install {
        println!(
            "tmux popup binding: bind-key a display-popup -E -w 80% -h 80% {} ui",
            quote(binary)
        );
    }
    Ok(())
}
