use super::{other, transport::failure};
use crate::{
    model::AgentState,
    rpc::{self, Outcome, Request, Response, Version},
    store::Store,
};
use serde_json::{Value, json};

pub fn empty_params(request: &Request) -> bool {
    request
        .params
        .as_ref()
        .is_none_or(|p| p.is_null() || p.as_object().is_some_and(|o| o.is_empty()))
}
pub fn handle(request: Request, store: &Store, key: &str, poll_ms: u64) -> Response {
    let id = Some(request.id.clone());
    let result = match request.method.as_str() {
        "hello" | "daemon.status" | "daemon.stop" if !empty_params(&request) => {
            return failure(id, rpc::INVALID_PARAMS, "expected empty params");
        }
        "hello" => Ok(
            json!({"rpc_version": rpc::RPC_VERSION, "daemon_version": env!("CARGO_PKG_VERSION"), "server_key": key}),
        ),
        "daemon.status" => {
            Ok(json!({"running":true,"pid":std::process::id(),"server_key":key,"poll_ms":poll_ms}))
        }
        "daemon.stop" => Ok(json!({"stopping":true})),
        "list" => list(&request, store),
        method => {
            let milestone = match method {
                "signal" => "M2",
                "attention" | "group.add" | "group.rm" | "group.ls" | "subscribe" => "M3",
                "history" | "wait" | "send" | "ack" | "name" => "M4",
                _ => return failure(id, rpc::METHOD_NOT_FOUND, "unknown method"),
            };
            return failure(
                id,
                rpc::METHOD_NOT_FOUND,
                format!("not implemented yet ({milestone})"),
            );
        }
    };
    match result {
        Ok(result) => Response {
            jsonrpc: Version::V2,
            id,
            outcome: Outcome::Success { result },
        },
        Err((code, message)) => failure(id, code, message),
    }
}
fn list(request: &Request, store: &Store) -> Result<Value, (i32, String)> {
    #[derive(serde::Deserialize, Default)]
    #[serde(deny_unknown_fields)]
    struct Filters {
        group: Option<String>,
        state: Option<AgentState>,
    }
    let filters: Filters = match request.params.as_ref().filter(|p| !p.is_null()) {
        None => Filters::default(),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| (rpc::INVALID_PARAMS, e.to_string()))?,
    };
    if filters.group.is_some() {
        return Err((rpc::METHOD_NOT_FOUND, "not implemented yet (M3)".into()));
    }
    let mut runs = store
        .list()
        .map_err(|e| (rpc::INTERNAL_ERROR, other(e).to_string()))?;
    if let Some(state) = filters.state {
        runs.retain(|run| run.state == state);
    }
    serde_json::to_value(runs).map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))
}
