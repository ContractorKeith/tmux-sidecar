//! Stdio MCP tools are a thin adapter over the per-tmux-server RPC socket.
mod tools;
use crate::{client, paths::Paths};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};
use std::{io, path::PathBuf, time::Duration};

struct Sidecar {
    socket: PathBuf,
    paths: Paths,
    allow_send: bool,
}
impl ServerHandler for Sidecar {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(tools::STATES)
            .with_server_info(Implementation::new(
                "tmux-sidecar",
                env!("CARGO_PKG_VERSION"),
            ))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::definitions(self.allow_send)
            .into_iter()
            .find(|t| t.name == name)
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools::definitions(self.allow_send),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let (method, params, timeout) = match tools::prepare(
            &request.name,
            request.arguments.unwrap_or_default(),
            self.allow_send,
        ) {
            Ok(call) => call,
            Err(message) => return Ok(tool_error(message).into()),
        };
        let result = client::request_with_timeout(
            &self.socket,
            &self.paths,
            method,
            params,
            true,
            Duration::from_secs(timeout),
        )
        .await;
        Ok(match result {
            Ok(value) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
            Err(error) => tool_error(error.to_string()),
        }
        .into())
    }
}
fn tool_error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

pub async fn run(socket: PathBuf, paths: Paths, allow_send: bool) -> io::Result<()> {
    let service = Sidecar {
        socket,
        paths,
        allow_send,
    }
    .serve(rmcp::transport::stdio())
    .await
    .map_err(io::Error::other)?;
    service.waiting().await.map_err(io::Error::other)?;
    Ok(())
}
