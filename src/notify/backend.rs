//! Socket lookups and notification commands run only on the notification task.
use super::command;
use std::{future::Future, io, path::PathBuf};
pub(super) trait Backend {
    fn current(&mut self, _event: &serde_json::Value) -> impl Future<Output = bool> + Send {
        async { true }
    }
    fn project_root(&mut self, _id: &str) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
    fn desktop(&mut self, message: &str) -> impl Future<Output = io::Result<()>> + Send;
    fn waiting(
        &mut self,
        pane: &str,
        start: u64,
        message: &str,
    ) -> impl Future<Output = io::Result<()>> + Send;
}
pub(super) struct Real {
    pub socket: PathBuf,
}
impl Backend for Real {
    async fn current(&mut self, event: &serde_json::Value) -> bool {
        let Some(run) = self.run(event["run_id"].as_str().unwrap_or("")).await else {
            return false;
        };
        run["ended_at"].is_null()
            && run["state"] == event["to"]
            && run["state_since"] == event["at"]
    }
    async fn project_root(&mut self, id: &str) -> Option<String> {
        self.run(id).await?["project_root"]
            .as_str()
            .map(str::to_owned)
    }
    async fn desktop(&mut self, message: &str) -> io::Result<()> {
        command::desktop(message).await
    }
    async fn waiting(&mut self, pane: &str, start: u64, message: &str) -> io::Result<()> {
        crate::tmux::waiting(&self.socket, pane, start, message).await
    }
}
impl Real {
    async fn run(&self, id: &str) -> Option<serde_json::Value> {
        let paths = crate::paths::current(&self.socket).ok()?;
        let runs = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            crate::client::request(&self.socket, &paths, "list", serde_json::json!({}), false),
        )
        .await
        .ok()?
        .ok()?;
        runs.as_array()?.iter().find(|run| run["id"] == id).cloned()
    }
}
