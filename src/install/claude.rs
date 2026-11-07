use serde_json::{Value, json};
use std::io;
pub const EVENTS: &[&str] = &[
    "Notification",
    "PermissionRequest",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
    "StopFailure",
];
pub fn edit(text: &str, install: bool, command: &str) -> io::Result<String> {
    edit_events(text, install, command, EVENTS)
}
pub(crate) fn edit_events(
    text: &str,
    install: bool,
    command: &str,
    events: &[&str],
) -> io::Result<String> {
    let mut value: Value = if text.is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)?
    };
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::other("settings must be an object"))?;
    if install && !object.contains_key("hooks") {
        object.insert("hooks".into(), json!({}));
    }
    if let Some(hooks) = object.get_mut("hooks") {
        let hooks = hooks
            .as_object_mut()
            .ok_or_else(|| io::Error::other("hooks must be an object"))?;
        let mut clean = events.to_vec();
        clean.extend(EVENTS);
        clean.push("StopCancelled");
        clean.sort_unstable();
        clean.dedup();
        for event in &clean {
            if install && events.contains(event) && !hooks.contains_key(*event) {
                hooks.insert((*event).into(), json!([]));
            }
            if let Some(entries) = hooks.get_mut(*event) {
                let entries = entries
                    .as_array_mut()
                    .ok_or_else(|| io::Error::other("hook event must be an array"))?;
                entries.retain_mut(|entry| {
                    let legacy = entry.get("_tmux_sidecar") == Some(&json!(1));
                    let mut drop_entry = false;
                    if let Some(handlers) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
                        let before = handlers.len();
                        handlers.retain(|handler| {
                            let command =
                                handler.get("command").and_then(Value::as_str).unwrap_or("");
                            let owned = handler.get("_tmux_sidecar") == Some(&json!(1))
                                || command.trim_end().ends_with(" # tmux-sidecar")
                                || legacy
                                    && (command.ends_with(" hook claude")
                                        || command.ends_with(" hook grok"));
                            !owned
                        });
                        drop_entry = (legacy || before > handlers.len()) && handlers.is_empty();
                    }
                    if legacy && let Some(entry) = entry.as_object_mut() {
                        entry.shift_remove("_tmux_sidecar");
                    }
                    !drop_entry
                });
                if install && events.contains(event) {
                    entries.push(json!({"hooks":[{"type":"command","command":format!("{command} # tmux-sidecar")}]}));
                }
                if entries.is_empty() {
                    hooks.shift_remove(*event);
                }
            }
        }
        if hooks.is_empty() {
            object.shift_remove("hooks");
        }
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}
