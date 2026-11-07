use crate::install;
use std::{io, path::Path};

// Inspect only our marked handlers; unrelated user handlers may appear in any order.
fn events(
    value: &serde_json::Value,
    agent: &str,
    binary: &str,
    required: &[&str],
) -> io::Result<()> {
    let hooks = value
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| io::Error::other("hooks missing"))?;
    let command = format!("{} hook {agent} # tmux-sidecar", install::quote(binary));
    for event in required {
        let groups = hooks
            .get(*event)
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other(format!("{event} missing")))?;
        let mut found = false;
        for group in groups {
            for handler in group
                .get("hooks")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
            {
                let text = handler
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                if text.trim_end().ends_with("# tmux-sidecar")
                    || handler.get("_tmux_sidecar") == Some(&serde_json::json!(1))
                    || group.get("_tmux_sidecar") == Some(&serde_json::json!(1))
                {
                    if text != command
                        || handler.get("type").and_then(serde_json::Value::as_str)
                            != Some("command")
                    {
                        return Err(io::Error::other(format!("{event} marked command drifted")));
                    }
                    found = true;
                }
            }
        }
        if !found {
            return Err(io::Error::other(format!("{event} marked command missing")));
        }
    }
    Ok(())
}
fn claude(home: &Path, file: &str, binary: &str, agent: &str, required: &[&str]) -> io::Result<()> {
    let text = std::fs::read_to_string(home.join(file))?;
    events(&serde_json::from_str(&text)?, agent, binary, required)
}
pub(super) fn check(home: &Path, binary: &Path, agent: &str) -> io::Result<String> {
    let binary = binary
        .to_str()
        .ok_or_else(|| io::Error::other("non-UTF-8 binary path"))?;
    match agent {
        "claude" => claude(
            home,
            ".claude/settings.json",
            binary,
            "claude",
            install::claude::EVENTS,
        )?,
        "grok" => {
            claude(
                home,
                ".grok/hooks/tmux-sidecar.json",
                binary,
                "grok",
                &["StopCancelled"],
            )?;
            if install::grok::shared(home)? {
                claude(
                    home,
                    ".claude/settings.json",
                    binary,
                    "claude",
                    install::claude::EVENTS,
                )?;
            } else {
                claude(
                    home,
                    ".grok/hooks/tmux-sidecar-common.json",
                    binary,
                    "grok",
                    install::claude::EVENTS,
                )?;
            }
        }
        "codex" => codex(
            &std::fs::read_to_string(home.join(".codex/config.toml"))?,
            binary,
        )?,
        "opencode" => {
            let before =
                std::fs::read_to_string(home.join(".config/opencode/plugins/tmux-sidecar.js"))?;
            if before != install::opencode::edit(&before, true, binary)? {
                return Err(io::Error::other("plugin missing or drifted"));
            }
        }
        _ => unreachable!(),
    }
    Ok("markers and current binary path verified".into())
}
fn codex(before: &str, binary: &str) -> io::Result<()> {
    let parsed = |s: &str| toml::from_str::<toml::Value>(s).map_err(io::Error::other);
    let value = parsed(before)?;
    let doc = before
        .parse::<toml_edit::DocumentMut>()
        .map_err(io::Error::other)?;
    let notify = doc.get("notify").and_then(toml_edit::Item::as_value);
    let marked = notify
        .and_then(|v| v.decor().suffix())
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.trim() == "# tmux-sidecar");
    let argv = value.get("notify").and_then(toml::Value::as_array);
    let direct = serde_json::json!([binary, "hook", "codex-notify"]);
    let direct = argv.is_some_and(|a| serde_json::to_value(a).ok().as_ref() == Some(&direct));
    let chained = argv.is_some_and(|a| {
        let arg = |i| a.get(i).and_then(toml::Value::as_str);
        arg(0) == Some("/bin/sh")
            && arg(1) == Some("-c")
            && arg(3) == Some("tmux-sidecar")
            && a.len() > 4
            && arg(2)
                == Some(&format!(
                    "for payload do :; done; {} hook codex-notify \"$payload\"; exec \"$@\"",
                    install::quote(binary)
                ))
    });
    if !marked || !(direct || chained) {
        return Err(io::Error::other(
            "Codex notify or inline hooks missing or drifted",
        ));
    }
    events(
        &serde_json::to_value(value)?,
        "codex",
        binary,
        install::codex::EVENTS,
    )?;
    Ok(())
}
