//! Page stored candidates, then resolve project roots off the RPC and SQLite tasks.
use super::{
    labels::Group,
    methods::{self, Filters},
    store_worker::StoreWorker,
    transport::{Call, failure},
};
use crate::{
    model::AgentRun,
    rpc::{self, Outcome, Response, Version},
    store::Store,
};
pub fn stored(
    store: &Store,
    filters: &Filters,
    group: Option<&Group>,
    before: Option<u64>,
    limit: Option<u32>,
) -> Result<Vec<AgentRun>, (i32, String)> {
    store
        .list_page(
            filters.all,
            filters.state,
            limit,
            before,
            group.and_then(Group::manual),
        )
        .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))
}
pub async fn serve(call: Call, worker: StoreWorker) {
    let id = call.request.id.clone();
    let result = async {
        let filters = methods::filters(&call.request)?;
        let group = Group::parse(filters.group.as_deref())?;
        let mut roots = session_board::project::ProjectRoots::default();
        let mut result = Vec::new();
        let mut before = None;
        loop {
            let page_limit = if filters.all {
                Some(if matches!(group, Some(Group::Project(_))) {
                    64
                } else {
                    filters.limit
                })
            } else {
                None
            };
            if filters.all && filters.limit == 0 {
                break;
            }
            let copied = Filters {
                group: None,
                state: filters.state,
                all: filters.all,
                limit: filters.limit,
            };
            let selected = group.clone();
            let runs = worker
                .read(move |store| stored(store, &copied, selected.as_ref(), before, page_limit))
                .await
                .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))??;
            let count = runs.len();
            before = runs
                .last()
                .map(|r| r.id[1..].parse::<u64>().expect("stored numeric run id"));
            let selected = group.clone();
            let (resolved, cache) = tokio::task::spawn_blocking(move || {
                let mut resolved = Vec::new();
                for mut run in runs {
                    let root = roots
                        .resolve(std::path::Path::new(&run.cwd))
                        .to_string_lossy()
                        .into_owned();
                    run.project_root = Some(root.clone());
                    if !matches!(&selected,Some(Group::Project(wanted)) if wanted!=&root) {
                        resolved.push(run);
                    }
                }
                (resolved, roots)
            })
            .await
            .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))?;
            roots = cache;
            result.extend(resolved);
            if !filters.all
                || !matches!(group, Some(Group::Project(_)))
                || result.len() >= filters.limit as usize
                || count < 64
            {
                break;
            }
        }
        if filters.all {
            result.truncate(filters.limit as usize);
        }
        let result = worker
            .with(move |store| {
                result
                    .into_iter()
                    .map(|run| {
                        let name = store.pane_name(run.server_start, &run.pane_id)?;
                        let mut value = serde_json::to_value(run)?;
                        value["pane_name"] = serde_json::json!(name);
                        Ok(value)
                    })
                    .collect::<std::io::Result<Vec<_>>>()
            })
            .await
            .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))?
            .map_err(|e| (rpc::INTERNAL_ERROR, e.to_string()))?;
        Ok(serde_json::json!(result))
    }
    .await;
    let response = match result {
        Ok(result) => Response {
            jsonrpc: Version::V2,
            id: Some(id),
            outcome: Outcome::Success { result },
        },
        Err((code, message)) => failure(Some(id), code, message),
    };
    let _ = call.reply.send(response);
}
