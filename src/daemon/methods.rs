use super::transport::failure;
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
type Prepared = Option<Result<crate::hook::signal::Signal, String>>;
pub fn prepare(request: &Request) -> Prepared {
    (request.method == "signal").then(|| {
        let signal: crate::hook::signal::Signal =
            serde_json::from_value(request.params.clone().unwrap_or(Value::Null))
                .map_err(|e| e.to_string())?;
        signal.validate().map_err(|e| e.to_string())?;
        Ok(signal)
    })
}
pub fn handle(request: Request, store: &Store, key: &str, poll_ms: u64) -> Response {
    let prepared = prepare(&request);
    handle_prepared(request, store, key, poll_ms, prepared, 0)
}
pub fn handle_prepared(
    request: Request,
    store: &Store,
    key: &str,
    poll_ms: u64,
    prepared: Prepared,
    server_start: u64,
) -> Response {
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
        "daemon.stop" => Ok(json!({"stopping":true,"pid":std::process::id()})),
        "list" => list(&request, store),
        "attention" | "history" | "ack" | "name" | "group.add" | "group.rm" | "group.ls"
        | "subscribe" => super::state_methods::handle(
            &request.method,
            request.params.clone(),
            store,
            server_start,
        ),
        "signal" => match prepared.expect("signal prepared before dispatch") {
            Err(e) => Err((rpc::INVALID_PARAMS, e)),
            Ok(signal) => store
                .signal(&signal)
                .map(|()| json!({}))
                .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string())),
        },
        "wait" | "send" => {
            return failure(
                id,
                rpc::INVALID_PARAMS,
                "requires asynchronous daemon dispatch",
            );
        }
        _ => return failure(id, rpc::METHOD_NOT_FOUND, "unknown method"),
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
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Filters {
    pub group: Option<String>,
    pub state: Option<AgentState>,
    #[serde(default)]
    pub all: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
}
pub(super) fn filters(request: &Request) -> Result<Filters, (i32, String)> {
    match request.params.as_ref().filter(|p| !p.is_null()) {
        None => Ok(Filters {
            group: None,
            state: None,
            all: false,
            limit: default_limit(),
        }),
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|e| (rpc::INVALID_PARAMS, e.to_string()))
        }
    }
}
fn list(request: &Request, store: &Store) -> Result<Value, (i32, String)> {
    let filters = filters(request)?;
    let group = super::labels::Group::parse(filters.group.as_deref())?;
    let runs = super::list::stored(
        store,
        &filters,
        group.as_ref(),
        None,
        filters.all.then_some(filters.limit),
    )?;
    serde_json::to_value(runs).map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))
}

fn default_limit() -> u32 {
    100
}
