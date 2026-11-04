use serde_json::{Value, json};
use std::io;
pub const EVENTS: &[&str] = &[
    "Notification",
    "PermissionRequest",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "StopCancelled",
    "SessionEnd",
    "StopFailure",
];
pub fn edit(text: &str, install: bool, command: &str) -> io::Result<String> {
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
        for event in EVENTS {
            if install && !hooks.contains_key(*event) {
                hooks.insert((*event).into(), json!([]));
            }
            if let Some(entries) = hooks.get_mut(*event) {
                let entries = entries
                    .as_array_mut()
                    .ok_or_else(|| io::Error::other("hook event must be an array"))?;
                entries.retain(|e| e.get("_tmux_sidecar") != Some(&json!(1)));
                if install {
                    entries.push(
                        json!({"_tmux_sidecar":1,"hooks":[{"type":"command","command":command}]}),
                    );
                }
                if entries.is_empty() {
                    hooks.remove(*event);
                }
            }
        }
        if hooks.is_empty() {
            object.remove("hooks");
        }
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}
