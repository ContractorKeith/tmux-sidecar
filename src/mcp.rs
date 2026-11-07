//! Stdio MCP tools are a thin adapter over the per-tmux-server RPC socket.
mod input;
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
    closed: tokio::sync::watch::Receiver<bool>,
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
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if self.get_tool(&request.name).is_none() {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            ));
        }
        let (method, params, timeout) = match tools::prepare(
            &request.name,
            request.arguments.unwrap_or_default(),
            self.allow_send,
        ) {
            Ok(call) => call,
            Err(message) => return Ok(tool_error(message).into()),
        };
        let mut closed = self.closed.clone();
        let call = async {
            if let Some(seconds) = timeout {
                let mut stream = client::open(&self.socket, &self.paths, true).await?;
                tokio::time::timeout(
                    Duration::from_secs(seconds + 2),
                    client::exchange(&mut stream, method, params),
                )
                .await
                .unwrap_or_else(|_| {
                    Err(io::Error::other(crate::rpc::RpcError {
                        code: crate::rpc::TIMEOUT,
                        message: format!("MCP wait exceeded {} s", seconds + 2),
                        data: None,
                    }))
                })
                .map_err(|error| client::connected_error(method, error))
            } else {
                // Share the CLI's ordinary and send budgets and outcome-unknown errors.
                client::request(&self.socket, &self.paths, method, params, true).await
            }
        };
        let result = tokio::select! {
            biased;
            _ = context.ct.cancelled() => return Ok(tool_error("cancelled: MCP request cancelled".into()).into()),
            _ = closed.wait_for(|closed| *closed) => return Ok(tool_error("cancelled: MCP stdin closed".into()).into()),
            result = call => result,
        };
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
    let (closed, receiver) = tokio::sync::watch::channel(false);
    let (reader, writer) = rmcp::transport::stdio();
    let service = Sidecar {
        socket,
        paths,
        allow_send,
        closed: receiver,
    }
    .serve((input::Input { reader, closed }, writer))
    .await
    .map_err(io::Error::other)?;
    service.waiting().await.map_err(io::Error::other)?;
    Ok(())
}
