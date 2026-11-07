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
        let mut cells: Vec<_> = columns
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
        if row["stale"] == true
            && let Some(index) = columns.iter().position(|key| *key == "state")
        {
            cells[index].push_str(" STALE");
        }
        writeln!(out, "{}", cells.join("\t"))?;
    }
    if let Some(reason) = rows
        .iter()
        .find(|r| r["stale"] == true)
        .and_then(|r| r["stale_reason"].as_str())
    {
        writeln!(out, "daemon can't reach tmux: {}", reason.escape_debug())?;
    }
    Ok(())
}

// Empty queues carry no row diagnostics; consult daemon health for human output.
pub async fn print_read(
    socket: &std::path::Path,
    paths: &crate::paths::Paths,
    method: &str,
    value: &Value,
    json: bool,
) -> io::Result<()> {
    let reason = if !json
        && matches!(method, "list" | "attention")
        && value.as_array().is_some_and(Vec::is_empty)
    {
        let health =
            crate::client::request(socket, paths, "daemon.status", serde_json::json!({}), false)
                .await?;
        health["poll_error"].as_str().map(str::to_owned)
    } else {
        None
    };
    print(method, value, json)?;
    if let Some(reason) = reason {
        writeln!(
            io::stdout().lock(),
            "daemon can't reach tmux: {}",
            reason.escape_debug()
        )?;
    }
    Ok(())
}
