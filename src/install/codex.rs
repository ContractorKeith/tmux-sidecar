use std::io;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, Value};
const MARKER: &str = "# tmux-sidecar";
pub(crate) const EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Interrupt",
    "SessionEnd",
];
fn marked(item: &Item) -> bool {
    item.as_value()
        .and_then(|v| v.decor().suffix())
        .and_then(|s| s.as_str())
        .is_some_and(|s| s.trim() == MARKER)
}
pub fn edit(text: &str, install: bool, chain: bool, binary: &str) -> io::Result<String> {
    let mut doc = text.parse::<DocumentMut>().map_err(io::Error::other)?;
    let ours = doc.get("notify").is_some_and(marked);
    if install && !ours && doc.contains_key("notify") && !chain {
        return Err(io::Error::other(
            "Codex notify already exists; use --chain to call it with the same argv after recording",
        ));
    }
    if install && !ours {
        let old = doc.get("notify").and_then(Item::as_array).cloned();
        if doc.contains_key("notify") && old.is_none() {
            return Err(io::Error::other("notify must be an array"));
        }
        let mut notify = Array::new();
        if let Some(old) = old {
            for arg in [
                "/bin/sh",
                "-c",
                &format!(
                    "for payload do :; done; {} hook codex-notify \"$payload\"; exec \"$@\"",
                    super::quote(binary)
                ),
                "tmux-sidecar",
            ] {
                notify.push(arg);
            }
            for arg in old.iter() {
                notify.push(arg.clone());
            }
        } else {
            for arg in [binary, "hook", "codex-notify"] {
                notify.push(arg);
            }
        }
        let mut notify = Value::Array(notify);
        if let Some(previous) = doc.get("notify").and_then(Item::as_value) {
            *notify.decor_mut() = previous.decor().clone();
        }
        notify.decor_mut().set_suffix(format!(" {MARKER}"));
        doc["notify"] = Item::Value(notify);
    } else if !install && ours {
        let old = doc["notify"]
            .as_array()
            .filter(|a| a.get(3).and_then(Value::as_str) == Some("tmux-sidecar"))
            .map(|a| a.iter().skip(4).cloned().collect::<Array>());
        if let Some(old) = old {
            let decor = doc["notify"].as_value().map(|v| v.decor().clone());
            let mut replacement = Value::Array(old);
            if let Some(decor) = decor {
                *replacement.decor_mut() = decor;
            }
            replacement.decor_mut().set_suffix("");
            doc["notify"] = Item::Value(replacement);
        } else {
            doc.remove("notify");
        }
    }
    if install && !doc.contains_key("hooks") {
        doc["hooks"] = Item::Table(Table::new());
    }
    if let Some(hooks) = doc.get_mut("hooks") {
        let hooks = hooks
            .as_table_mut()
            .ok_or_else(|| io::Error::other("hooks must be a table"))?;
        for event in EVENTS.iter().chain(std::iter::once(&"Stop")) {
            if install && *event != "Stop" && !hooks.contains_key(event) {
                hooks[event] = Item::ArrayOfTables(ArrayOfTables::new());
            }
            if let Some(groups) = hooks.get_mut(event)
                && super::codex_groups::edit(groups, install && *event != "Stop", binary)?
            {
                hooks.remove(event);
            }
        }
        if hooks.is_empty() {
            doc.remove("hooks");
        }
    }
    let edited = doc.to_string();
    Ok(if text.contains("\r\n") {
        edited.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        edited
    })
}
