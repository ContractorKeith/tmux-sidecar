//! Resolve computed project groups on a blocking task for each list request.
use super::{
    methods,
    store_worker::StoreWorker,
    transport::{Call, failure},
};
use crate::rpc;
use serde_json::{Value, json};
pub async fn serve(call: Call, worker: StoreWorker, key: String, poll_ms: u64) {
    let mut request = call.request;
    let filters = match methods::filters(&request) {
        Ok(filters) => filters,
        Err((code, message)) => {
            let _ = call.reply.send(failure(Some(request.id), code, message));
            return;
        }
    };
    let group = filters.group;
    let limit = filters.limit;
    let all = filters.all;
    // Validate the original filters before expanding a grouped query's limit.
    let prepared = methods::prepare(&request);
    if let Some(params) = request.params.as_mut().and_then(Value::as_object_mut) {
        params.remove("group");
        if group.is_some() && all {
            params.insert("limit".into(), json!(u32::MAX));
        }
    }
    let id = request.id.clone();
    let result = worker
        .with(move |store| {
            let response = methods::handle_prepared(request, store, &key, poll_ms, prepared);
            let mut members = Vec::new();
            if let Some(group) = &group {
                for run in store.list()? {
                    if store.group_contains(group, &run)? {
                        members.push(run.id);
                    }
                }
            }
            Ok::<_, std::io::Error>((response, members, group))
        })
        .await;
    let response = match result {
        Ok(Ok((mut response, members, group))) => {
            let resolved = tokio::task::spawn_blocking(move || {
                if let rpc::Outcome::Success { result } = &mut response.outcome
                    && let Some(runs) = result.as_array_mut()
                {
                    let mut roots = session_board::project::ProjectRoots::default();
                    for run in runs.iter_mut() {
                        if let Some(cwd) = run["cwd"].as_str() {
                            let root = roots.resolve(std::path::Path::new(cwd));
                            run["project_root"] = json!(root);
                        }
                    }
                    if let Some(group) = group {
                        runs.retain(|r| {
                            r["project_root"] == group
                                || r["id"]
                                    .as_str()
                                    .is_some_and(|id| members.iter().any(|m| m == id))
                        });
                        if all {
                            runs.truncate(limit as usize);
                        }
                    }
                }
                response
            })
            .await;
            resolved.unwrap_or_else(|e| failure(Some(id), rpc::INTERNAL_ERROR, e.to_string()))
        }
        Ok(Err(e)) | Err(e) => failure(Some(id), rpc::INTERNAL_ERROR, e.to_string()),
    };
    let _ = call.reply.send(response);
}
