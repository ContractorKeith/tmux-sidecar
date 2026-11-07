//! Long-lived operations run independently of the RPC dispatcher and SQLite owner.
use super::{
    store_worker::StoreWorker,
    transport::{Call, failure},
};
use crate::{
    model::{AgentRun, AgentState, RunSelector},
    rpc,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
type Result<T> = std::result::Result<T, (i32, String)>;
fn internal(e: impl std::fmt::Display) -> (i32, String) {
    (rpc::INTERNAL_ERROR, e.to_string())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wait {
    sel: String,
    until: AgentState,
    timeout: Option<u64>,
}
fn parse<T: serde::de::DeserializeOwned>(params: Option<Value>) -> Result<T> {
    serde_json::from_value(params.unwrap_or(json!({})))
        .map_err(|e| (rpc::INVALID_PARAMS, e.to_string()))
}
async fn select(worker: &StoreWorker, sel: &str, start: u64) -> Result<AgentRun> {
    let selector: RunSelector = sel
        .parse()
        .map_err(|e: &str| (rpc::INVALID_PARAMS, e.into()))?;
    worker
        .read(move |s| s.get(selector, start))
        .await
        .map_err(internal)?
        .map_err(internal)?
        .ok_or((rpc::NOT_FOUND, format!("run {sel} not found")))
}
pub async fn serve(mut call: Call, worker: StoreWorker, socket: PathBuf, start: u64) {
    let id = Some(call.request.id);
    let result = if call.request.method == "wait" {
        match parse(call.request.params) {
            Ok(params) => tokio::select! {
                result = wait(&worker,params,start) => result,
                _ = call.reply.closed() => return,
            },
            Err(e) => Err(e),
        }
    } else {
        match parse(call.request.params) {
            Ok(params) => sending::send(&worker, socket, params, start).await,
            Err(e) => Err(e),
        }
    };
    let response = match result {
        Ok(result) => rpc::Response {
            jsonrpc: rpc::Version::V2,
            id,
            outcome: rpc::Outcome::Success { result },
        },
        Err((code, message)) => failure(id, code, message),
    };
    let _ = call.reply.send(response);
}
async fn wait(worker: &StoreWorker, params: Wait, start: u64) -> Result<Value> {
    if !matches!(
        params.until,
        AgentState::Waiting | AgentState::Blocked | AgentState::Done
    ) {
        return Err((
            rpc::INVALID_PARAMS,
            "until must be waiting, blocked or done".into(),
        ));
    }
    let deadline = params
        .timeout
        .map(|secs| {
            tokio::time::Instant::now()
                .checked_add(Duration::from_secs(secs))
                .ok_or((rpc::INVALID_PARAMS, "timeout is too large".into()))
        })
        .transpose()?;
    // Subscribe before reading, so a change between the snapshot and recv cannot be lost.
    let mut events = worker.events.subscribe();
    let waiting = async {
        let run = select(worker, &params.sel, start).await?;
        let id = run.id.clone();
        if run.state == params.until {
            return Ok(json!({"run_id":id,"state":run.state,"at":run.state_since}));
        }
        if run.ended_at.is_some() || run.state == AgentState::Done {
            return Err((
                rpc::ERROR,
                format!(
                    "run {id} ended in {}",
                    format!("{:?}", run.state).to_ascii_lowercase()
                ),
            ));
        }
        loop {
            match events.recv().await {
                Ok(event) if event["run_id"] == id => {
                    // Inspect the committed transition, including transient states already followed by another.
                    let state: AgentState =
                        serde_json::from_value(event["to"].clone()).map_err(internal)?;
                    if state == params.until {
                        return Ok(json!({"run_id":id,"state":state,"at":event["at"]}));
                    }
                    if state == AgentState::Done {
                        return Err((
                            rpc::ERROR,
                            format!(
                                "run {id} ended in {}",
                                format!("{state:?}").to_ascii_lowercase()
                            ),
                        ));
                    }
                }
                Ok(_) => (),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    return Err((rpc::ERROR, "wait missed state changes".into()));
                }
                Err(_) => return Err((rpc::ERROR, "daemon stopped".into())),
            }
        }
    };
    match deadline {
        Some(deadline) => tokio::time::timeout_at(deadline, waiting)
            .await
            .map_err(|_| (rpc::TIMEOUT, format!("wait for {} timed out", params.sel)))?,
        None => waiting.await,
    }
}
#[path = "send.rs"]
mod sending;

#[cfg(test)]
#[path = "action_tests.rs"]
mod tests;
