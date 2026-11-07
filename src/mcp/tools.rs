use rmcp::model::Tool;
use serde_json::{Map, Value, json};

pub const STATES: &str = "An agent run is one coding-agent process in one tmux pane, from start to exit. States: working = processing a turn; waiting = finished a turn and wants the next prompt; blocked = mid-turn at a permission or approval request; idle = a read waiting run inactive for 30 minutes by default; error = failed turn; done = process exited or run ended. Select a run by r42 or the current run in pane %7. send_prompt only works on waiting runs.";

pub fn definitions(allow_send: bool) -> Vec<Tool> {
    let selector =
        json!({"type":"string","description":"Agent run id (r42) or current pane selector (%7)."});
    let states = json!({"type":"string","enum":["working","waiting","blocked","idle","error","done"],"description":"working: processing; waiting: wants next prompt; blocked: needs approval; idle: read waiting run inactive; error: failed turn; done: ended run."});
    let mut tools = vec![
        definition(
            "list_runs",
            "List active agent runs, optionally filtered by group and state.",
            json!({
                "group":{"type":"string","description":"manual:NAME or project:/root; bare names and paths also accepted."}, "state":states
            }),
            &[],
        ),
        definition(
            "attention",
            "Unread active agent runs needing a person: blocked first, then error, then waiting; oldest first. Does not mark them read.",
            json!({}),
            &[],
        ),
        definition(
            "run_history",
            "Read an agent run's signals and state changes, newest first. Default limit 100.",
            json!({
                "selector":selector,"limit":{"type":"integer","minimum":0,"maximum":u32::MAX,"description":"Maximum history rows, default 100; newest first."}
            }),
            &["selector"],
        ),
        definition(
            "wait_for_state",
            "Wait for an agent run to reach waiting, blocked or done. Timeout defaults to 600 seconds and cannot exceed 600; timeout is a tool error.",
            json!({
                "selector":selector,"state":{"type":"string","enum":["waiting","blocked","done"],"description":"Target: wants next prompt, needs approval, or ended run."},
                "timeout":{"type":"integer","minimum":0,"maximum":600,"default":600,"description":"Maximum seconds to wait (0 through 600)."}
            }),
            &["selector", "state"],
        ),
    ];
    if allow_send {
        tools.push(definition("send_prompt", "Paste text and Enter into a waiting agent run. Refuses working, blocked, idle, error and done runs; cannot override the guard. Only exposed with --allow-send.", json!({
            "selector":selector,"text":{"type":"string","description":"Prompt text pasted literally and followed by Enter into a waiting run."}
        }), &["selector", "text"]));
    }
    tools
}
fn definition(name: &'static str, description: &str, properties: Value, required: &[&str]) -> Tool {
    let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    Tool::new(
        name,
        description.to_string(),
        schema.as_object().expect("object schema").clone(),
    )
}

/// Validate even when an MCP client bypasses the advertised input schema.
pub fn prepare(
    name: &str,
    mut args: Map<String, Value>,
    allow_send: bool,
) -> Result<(&'static str, Value, u64), String> {
    let definition = definitions(allow_send)
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("unknown tool: {name}"))?;
    let schema = &definition.input_schema;
    let properties = schema["properties"].as_object().expect("properties");
    for key in schema["required"].as_array().expect("required") {
        if !args.contains_key(key.as_str().expect("key")) {
            return Err(format!("missing {key}"));
        }
    }
    for (key, value) in &args {
        let rule = properties
            .get(key)
            .ok_or_else(|| format!("unexpected parameter: {key}"))?;
        let valid = match rule["type"].as_str() {
            Some("string") => value.is_string(),
            Some("integer") => value
                .as_u64()
                .is_some_and(|n| n <= rule["maximum"].as_u64().unwrap_or(u64::MAX)),
            _ => false,
        };
        if !valid
            || rule["enum"]
                .as_array()
                .is_some_and(|choices| !choices.contains(value))
        {
            return Err(format!("invalid {key}: {value}"));
        }
    }
    if let Some(selector) = args.remove("selector") {
        selector
            .as_str()
            .expect("validated string")
            .parse::<crate::model::RunSelector>()
            .map_err(str::to_string)?;
        args.insert("sel".into(), selector);
    }
    let mut timeout = 2;
    let method = match name {
        "list_runs" => "list",
        "attention" => "attention",
        "run_history" => "history",
        "wait_for_state" => {
            let state = args.remove("state").expect("required state");
            args.insert("until".into(), state);
            let seconds = args
                .entry("timeout")
                .or_insert(json!(600))
                .as_u64()
                .expect("validated timeout");
            timeout = seconds + 2;
            "wait"
        }
        "send_prompt" => "send",
        _ => unreachable!(),
    };
    Ok((method, Value::Object(args), timeout))
}
