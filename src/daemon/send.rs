use super::super::store_worker::StoreWorker;
use super::{Result, internal, select};
use crate::{
    model::AgentState,
    rpc,
    tmux::{RealTmux, Socket},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::time::Instant;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Send {
    sel: String,
    text: String,
    #[serde(default)]
    force: bool,
}
fn failure(error: std::io::Error) -> (i32, String) {
    if crate::tmux::paste_outcome_unknown(&error) {
        (rpc::OUTCOME_UNKNOWN, error.to_string())
    } else if error.kind() == std::io::ErrorKind::TimedOut {
        (rpc::TIMEOUT, error.to_string())
    } else {
        internal(error)
    }
}
pub(super) async fn send(
    worker: &StoreWorker,
    socket: PathBuf,
    params: Send,
    start: u64,
) -> Result<Value> {
    if params.text.is_empty() {
        return Err((rpc::INVALID_PARAMS, "empty prompt".into()));
    }
    if params.text.contains('\0') {
        return Err((rpc::INVALID_PARAMS, "text contains NUL".into()));
    }
    let deadline = Instant::now() + Duration::from_secs(rpc::SEND_TIMEOUT_SECS);
    let tmux = RealTmux {
        socket: Socket::Path(socket),
    };
    let prepared = async {
        let selected = select(worker, &params.sel, start).await?;
        // Slow probes run before the lock; cancellation kills them before any paste can start.
        let scan = tmux.scan_before(deadline).await.map_err(failure)?;
        let processes = crate::proc::PsProcessTree::snapshot_before(deadline)
            .await
            .map_err(failure)?;
        let changes = crate::engine::detect_scan(
            &scan.panes,
            &scan.skipped,
            &processes,
            std::slice::from_ref(&selected),
            super::super::now(),
        );
        if selected.server_start != start
            || changes.end.contains(&selected.id)
            || scan.skipped.contains(&selected.pane_id)
        {
            return Err((rpc::NOT_FOUND, "agent run is no longer live".into()));
        }
        let guard = worker.lock_run(&selected.id).await;
        let run = select(worker, &selected.id, start).await?;
        if run.ended_at.is_some() {
            return Err((rpc::NOT_FOUND, format!("run {} has ended", run.id)));
        }
        if !params.force && !matches!(run.state, AgentState::Waiting | AgentState::Idle) {
            return Err((
                rpc::REFUSED,
                format!(
                    "run {} is {} (requires waiting or idle)",
                    run.id,
                    format!("{:?}", run.state).to_ascii_lowercase()
                ),
            ));
        }
        Ok((run, guard))
    };
    let (run, guard) = tokio::time::timeout_at(deadline, prepared)
        .await
        .map_err(|_| (rpc::TIMEOUT, "send exceeded 3 s before paste".into()))??;
    let live = run.clone();
    // Always join terminal work before replying. Its absolute deadline is checked
    // before every command, so even a delayed blocking task cannot paste after timeout.
    let _guard = tokio::task::spawn_blocking(move || {
        if Instant::now() >= deadline {
            return Err((rpc::TIMEOUT, "send deadline exceeded before paste".into()));
        }
        if !i32::try_from(live.agent_pid).is_ok_and(crate::proc::alive) {
            return Err((rpc::NOT_FOUND, "agent run is no longer live".into()));
        }
        tmux.paste_before(&live.pane_id, &params.text, deadline.into_std())
            .map_err(failure)?;
        tmux.enter_before(&live.pane_id, deadline.into_std())
            .map_err(|e| {
                (
                    rpc::OUTCOME_UNKNOWN,
                    format!("outcome unknown after paste: {e}"),
                )
            })?;
        Ok(guard)
    })
    .await
    .map_err(internal)??;
    tokio::time::timeout_at(
        deadline,
        worker.read(move |s| s.sent(&run, super::super::now())),
    )
    .await
    .map_err(|_| {
        (
            rpc::OUTCOME_UNKNOWN,
            "outcome unknown after paste: send deadline exceeded".into(),
        )
    })?
    .map_err(|e| {
        (
            rpc::OUTCOME_UNKNOWN,
            format!("outcome unknown after paste: {e}"),
        )
    })?
    .map_err(|e| {
        (
            rpc::OUTCOME_UNKNOWN,
            format!("outcome unknown after paste: {e}"),
        )
    })?;
    Ok(json!({}))
}
