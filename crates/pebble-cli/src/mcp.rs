//! Bounded RMCP server over standard input and standard output.

use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use pebble_core::service::PebbleService;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ErrorCode, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use tokio::sync::Semaphore;

use crate::mcp_tools::{self, ToolError};
use crate::mcp_transport::BoundedTransport;

const MAX_SEARCHES: usize = 8;
const MAX_BLOCKING_OPERATIONS: usize = 8;
const SERVICE_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(750);
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(750);

#[derive(Clone)]
struct McpServer {
    service: PebbleService,
    searches: Arc<Semaphore>,
    storage: Arc<Semaphore>,
}

impl McpServer {
    fn new(service: PebbleService) -> Self {
        Self {
            service,
            searches: Arc::new(Semaphore::new(MAX_SEARCHES)),
            storage: Arc::new(Semaphore::new(MAX_BLOCKING_OPERATIONS)),
        }
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("pebble", pebble_core::VERSION))
            .with_instructions("Local model-free repository evidence. No network access.")
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(mcp_tools::tools())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        mcp_tools::tools()
            .into_iter()
            .find(|tool| tool.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let response_budget = (request.name == mcp_tools::SEARCH_TOOL)
            .then(|| mcp_tools::requested_budget(request.arguments.as_ref()));
        let search_permit = if request.name == mcp_tools::SEARCH_TOOL {
            Some(acquire(&self.searches, &context).await?)
        } else {
            None
        };
        let storage_permit = acquire(&self.storage, &context).await?;
        let service = self.service.clone();
        let name = request.name.into_owned();
        let arguments = request.arguments;
        let mut task = tokio::task::spawn_blocking(move || {
            let _search_permit = search_permit;
            let _storage_permit = storage_permit;
            mcp_tools::execute(&service, &name, arguments)
        });
        tokio::select! {
            result = &mut task => map_join(result, &context.id, response_budget.flatten()),
            () = context.ct.cancelled() => {
                task.abort();
                Err(cancelled())
            }
        }
    }
}

async fn acquire(
    semaphore: &Arc<Semaphore>,
    context: &RequestContext<RoleServer>,
) -> Result<tokio::sync::OwnedSemaphorePermit, ErrorData> {
    tokio::select! {
        permit = semaphore.clone().acquire_owned() => {
            permit.map_err(|_| ErrorData::internal_error("operation limiter closed", None))
        }
        () = context.ct.cancelled() => Err(cancelled()),
    }
}

fn map_join(
    result: Result<Result<serde_json::Value, ToolError>, tokio::task::JoinError>,
    id: &rmcp::model::RequestId,
    response_budget: Option<u32>,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(Ok(value)) => crate::mcp_response::success(value, id, response_budget),
        Ok(Err(ToolError::Invalid(error))) => Err(ErrorData::invalid_params(bounded(&error), None)),
        Ok(Err(ToolError::Service(error))) => Ok(crate::mcp_response::structured_error(
            serde_json::json!({"error": bounded(&error.to_string())}),
        )),
        Ok(Err(ToolError::Unknown)) => Err(ErrorData::new(
            ErrorCode::METHOD_NOT_FOUND,
            "Unknown tool",
            None,
        )),
        Err(error) if error.is_cancelled() => Err(cancelled()),
        Err(_) => Err(ErrorData::internal_error(
            "blocking operation panicked",
            None,
        )),
    }
}

fn cancelled() -> ErrorData {
    ErrorData::new(ErrorCode(-32_800), "Request cancelled", None)
}

fn bounded(value: &str) -> String {
    let mut end = value.len().min(2_048);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_owned()
}

pub fn run(service: PebbleService) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let oversized = Arc::new(AtomicBool::new(false));
    let result = runtime.block_on(serve(service, oversized.clone()));
    if oversized.load(Ordering::Acquire) {
        eprintln!("pebble: MCP request exceeds 1 MiB");
        runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP request exceeds 1 MiB",
        ));
    }
    runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
    result
}

async fn serve(service: PebbleService, oversized: Arc<AtomicBool>) -> io::Result<()> {
    let (transport, connection_end) =
        BoundedTransport::new(tokio::io::stdin(), tokio::io::stdout(), oversized);
    let mut running = McpServer::new(service)
        .serve(transport)
        .await
        .map_err(io::Error::other)?;
    connection_end.wait().await;
    let failed = connection_end.failed();
    running.cancellation_token().cancel();
    running
        .close_with_timeout(SERVICE_SHUTDOWN_TIMEOUT)
        .await
        .map_err(io::Error::other)?;
    if failed {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "MCP transport failed",
        ));
    }
    Ok(())
}
