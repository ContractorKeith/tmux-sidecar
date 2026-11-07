use serde_json::Value;
use std::io::{self, Write};
pub fn print(method: &str, value: &Value, json: bool) -> io::Result<()> {
    let mut out = io::stdout().lock();
    if json {
        return writeln!(out, "{value}");
    }
    let columns: &[&str] = match method {
        "list" | "attention" => &["id", "pane_id", "agent", "state", "agent_pid", "cwd"],
        "wait" => &["run_id", "state", "at"],
        "history" => &["id", "at", "source", "event", "detail"],
        "group.ls" => &["name", "server_start", "pane_id"],
        "state_changed" => &["run_id", "from", "to", "at"],
        "daemon.status" => &["running", "pid", "server_key"],
        _ => return writeln!(out, "{method}: ok"),
    };
    if method != "state_changed" {
        writeln!(out, "{}", columns.join("\t").to_uppercase())?;
    }
    let rows = value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(std::slice::from_ref(value));
    for row in rows {
        let cells: Vec<_> = columns
            .iter()
            .map(|key| match &row[*key] {
                Value::Null => String::new(),
                Value::String(s) => s
                    .chars()
                    .map(|c| {
                        if c.is_control() {
                            c.escape_default().to_string()
                        } else {
                            c.to_string()
                        }
                    })
                    .collect(),
                other => other.to_string(),
            })
            .collect();
        writeln!(out, "{}", cells.join("\t"))?;
    }
    Ok(())
}
