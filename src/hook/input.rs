//! Drain the producer before inspecting context; keep only a bounded payload preview.
use serde_json::Value;
use std::io::{self, Read};
const INPUT_LIMIT: usize = 64 * 1024 * 1024;
pub fn read(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 64 * 1024];
    let mut oversized = false;
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count <= INPUT_LIMIT {
            bytes.extend_from_slice(&chunk[..count]);
        } else {
            oversized = true;
        }
    }
    if oversized {
        Err(io::Error::other(
            "hook input exceeds 64 MiB (drained to EOF)",
        ))
    } else {
        Ok(bytes)
    }
}
fn prefix(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn trim(value: &mut Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        *value = Value::String("…truncated".into());
        return true;
    }
    match value {
        Value::String(text) => {
            let limit = (*budget).min(4096);
            let changed = text.len() > limit;
            if changed {
                *text = format!("{}…truncated", prefix(text, limit));
            }
            *budget = budget.saturating_sub(text.len());
            changed
        }
        Value::Array(items) => {
            let mut changed = items.len() > 64;
            items.truncate(64);
            for item in items.iter_mut() {
                changed |= trim(item, budget);
            }
            if changed {
                items.push(Value::String("…truncated".into()));
            }
            changed
        }
        Value::Object(object) => {
            let mut count = 0;
            let mut changed = false;
            object.retain(|key, value| {
                count += 1;
                if count > 64 || key.len() > 4096 || *budget == 0 {
                    changed = true;
                    return false;
                }
                *budget = budget.saturating_sub(key.len() + 8);
                changed |= trim(value, budget);
                true
            });
            changed
        }
        _ => {
            *budget = budget.saturating_sub(16);
            false
        }
    }
}
pub fn compact(mut payload: Value) -> io::Result<Value> {
    let changed = trim(&mut payload, &mut (48 * 1024));
    if changed && let Some(object) = payload.as_object_mut() {
        object.insert("_tmux_sidecar_truncated".into(), true.into());
    }
    let serialized = serde_json::to_string(&payload)?;
    if serialized.len() >= 256 * 1024 {
        return Ok(
            serde_json::json!({"_tmux_sidecar_truncated":true,"preview":format!("{}…truncated",prefix(&serialized,32*1024))}),
        );
    }
    Ok(payload)
}
