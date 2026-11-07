//! SQLite-only RPC operations. Project resolution is prepared outside this module.
use crate::{model::AgentRun, rpc, store::Store};
use serde::Deserialize;
use serde_json::{Value, json};
pub type Result = std::result::Result<Value, (i32, String)>;
fn internal(e: std::io::Error) -> (i32, String) {
    (rpc::INTERNAL_ERROR, e.to_string())
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    sel: Option<String>,
    selectors: Option<Vec<String>>,
    group: Option<String>,
    name: Option<String>,
    limit: Option<u32>,
}
pub fn handle(method: &str, params: Option<Value>, store: &Store, server_start: u64) -> Result {
    let allowed: &[&str] = match method {
        "attention" | "subscribe" | "group.ls" => &[],
        "history" => &["sel", "limit"],
        "name" => &["sel", "name"],
        "ack" => &["sel", "selectors"],
        _ => &["group", "sel", "selectors"],
    };
    if params
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err((rpc::INVALID_PARAMS, "unexpected parameter".into()));
    }
    let params: Params =
        serde_json::from_value(params.filter(|v| !v.is_null()).unwrap_or(json!({})))
            .map_err(|e| (rpc::INVALID_PARAMS, e.to_string()))?;
    let active = store.active().map_err(internal)?;
    let select = |sel: &str| -> std::result::Result<AgentRun, (i32, String)> {
        let selector = sel
            .parse()
            .map_err(|e: &str| (rpc::INVALID_PARAMS, e.into()))?;
        store
            .get(selector, server_start)
            .map_err(internal)?
            .ok_or((rpc::NOT_FOUND, format!("run {sel} not found")))
    };
    match method {
        "attention" => {
            let mut runs: Vec<_> = active
                .into_iter()
                .filter(|r| {
                    matches!(
                        r.state,
                        crate::model::AgentState::Blocked
                            | crate::model::AgentState::Error
                            | crate::model::AgentState::Waiting
                    ) && r.read_at.is_none_or(|at| at < r.state_since)
                })
                .collect();
            runs.sort_by_key(|r| {
                (
                    match r.state {
                        crate::model::AgentState::Blocked => 0,
                        crate::model::AgentState::Error => 1,
                        _ => 2,
                    },
                    r.state_since,
                    r.id[1..].parse::<u64>().expect("stored numeric run id"),
                )
            });
            Ok(json!(runs))
        }
        "group.ls" => store.groups().map_err(internal),
        "subscribe" => Ok(json!({"subscribed":true})),
        "history" | "name" => {
            let run = select(
                params
                    .sel
                    .as_deref()
                    .ok_or((rpc::INVALID_PARAMS, "sel is required".into()))?,
            )?;
            if method == "history" {
                store
                    .history(&run, params.limit.unwrap_or(100))
                    .map_err(internal)
            } else {
                if let Some(name) = params.name.as_deref() {
                    super::labels::validate(name)?;
                }
                store
                    .name_pane(run.server_start, &run.pane_id, params.name.as_deref())
                    .map_err(internal)?;
                Ok(json!({}))
            }
        }
        "ack" | "group.add" | "group.rm" => {
            let selectors = params
                .selectors
                .or_else(|| params.sel.map(|s| vec![s]))
                .filter(|s| !s.is_empty())
                .ok_or((rpc::INVALID_PARAMS, "selectors are required".into()))?;
            let runs = selectors
                .iter()
                .map(|s| select(s))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if method == "ack" {
                for run in runs {
                    store.ack(&run, super::now()).map_err(internal)?;
                }
            } else {
                let group = params
                    .group
                    .filter(|s| !s.is_empty())
                    .ok_or((rpc::INVALID_PARAMS, "group is required".into()))?;
                super::labels::validate(&group)?;
                store
                    .group_edit(&group, &runs, method == "group.add")
                    .map_err(internal)?;
            }
            Ok(json!({}))
        }
        _ => unreachable!(),
    }
}
