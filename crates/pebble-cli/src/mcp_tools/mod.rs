//! MCP tool schemas and synchronous service adaptation.
mod plan2;

use pebble_core::domain::{Citation, RepositoryId, WorktreeRevision};
use pebble_core::retrieval::SearchRequest;
use pebble_core::service::{PebbleService, ServiceError};
use rmcp::model::JsonObject;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
pub const SEARCH_TOOL: &str = "search";

pub fn tools() -> Vec<rmcp::model::Tool> {
    crate::mcp_schemas::tools()
}
pub fn execute(
    service: &PebbleService,
    name: &str,
    arguments: Option<JsonObject>,
) -> Result<Value, ToolError> {
    let value = Value::Object(arguments.unwrap_or_default());
    match name {
        "repository_init" => {
            let request: PathRequest = parse(value)?;
            crate::mcp_validation::path(&request.repository)?;
            serialize(service.initialize(Path::new(&request.repository))?)
        }
        "repository_register" => {
            let request: RegisterRequest = parse(value)?;
            crate::mcp_validation::path(&request.repository)?;
            serialize(service.register(Path::new(&request.repository), request.alternate_worktree)?)
        }
        "repository_index" => {
            let request: PathRequest = parse(value)?;
            crate::mcp_validation::path(&request.repository)?;
            serialize(service.index(Path::new(&request.repository))?)
        }
        SEARCH_TOOL => {
            let request: SearchArguments = parse(value)?;
            crate::mcp_validation::search(
                &request.query,
                &request.repository,
                request.budget_tokens,
                request.max_results,
                request.revision.as_deref(),
                request.path_prefix.as_deref(),
                request.language.as_deref(),
                &request.kinds,
            )?;
            search(service, request)
        }
        "evidence_read" => {
            let request: ReadArguments = parse(value)?;
            crate::mcp_validation::read(
                &request.repository,
                &request.revision,
                &request.path,
                request.start_line,
                request.end_line,
            )?;
            read(service, request)
        }
        "index_health" => {
            let request: RepositoryRequest = parse(value)?;
            crate::mcp_validation::id("repository", &request.repository)?;
            serialize(service.health(&repository(&request.repository)?)?)
        }
        "trace_list" => {
            let request: TraceRequest = parse(value)?;
            crate::mcp_validation::trace(&request.repository, request.limit)?;
            serialize(service.traces(&repository(&request.repository)?, request.limit)?)
        }
        "projection_rebuild" => {
            let request: PathRequest = parse(value)?;
            crate::mcp_validation::path(&request.repository)?;
            serialize(service.rebuild(Path::new(&request.repository))?)
        }
        _ => plan2::execute_plan2(service, name, value),
    }
}
pub fn requested_budget(arguments: Option<&JsonObject>) -> Option<u32> {
    arguments
        .and_then(|arguments| arguments.get("budget_tokens"))
        .map_or(Some(default_budget()), |value| {
            value.as_u64().and_then(|value| u32::try_from(value).ok())
        })
}
#[derive(Debug)]
pub enum ToolError {
    Invalid(String),
    Service(ServiceError),
    Unknown,
}
impl From<ServiceError> for ToolError {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathRequest {
    repository: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterRequest {
    repository: String,
    #[serde(default)]
    alternate_worktree: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryRequest {
    repository: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceRequest {
    repository: String,
    #[serde(default = "default_trace_limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    query: String,
    repository: String,
    #[serde(default = "default_budget")]
    budget_tokens: u32,
    #[serde(default = "default_results")]
    max_results: usize,
    revision: Option<String>,
    path_prefix: Option<String>,
    language: Option<String>,
    #[serde(default)]
    kinds: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArguments {
    repository: String,
    revision: String,
    path: String,
    start_line: u32,
    end_line: u32,
}
fn search(service: &PebbleService, arguments: SearchArguments) -> Result<Value, ToolError> {
    let repository = repository(&arguments.repository)?;
    let mut request = SearchRequest::new(arguments.query).map_err(invalid)?;
    request = request
        .with_repository(arguments.repository)
        .map_err(invalid)?
        .with_budget_tokens(arguments.budget_tokens)
        .map_err(invalid)?
        .with_max_results(arguments.max_results)
        .map_err(invalid)?;
    if let Some(value) = arguments.revision {
        request = request.with_revision(value).map_err(invalid)?;
    }
    if let Some(value) = arguments.path_prefix {
        request = request.with_path_prefix(value).map_err(invalid)?;
    }
    if let Some(value) = arguments.language {
        request = request.with_language(value).map_err(invalid)?;
    }
    if !arguments.kinds.is_empty() {
        request = request.with_kinds(arguments.kinds).map_err(invalid)?;
    }
    let response = service.search(&repository, request)?;
    serialize(response.packet())
}
fn read(service: &PebbleService, arguments: ReadArguments) -> Result<Value, ToolError> {
    let citation = Citation::new(
        repository(&arguments.repository)?,
        revision(&arguments.revision)?,
        arguments.path,
        arguments.start_line,
        arguments.end_line,
    )
    .map_err(invalid)?;
    serialize(service.read(citation)?)
}
fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, ToolError> {
    serde_json::from_value(value).map_err(invalid)
}
fn serialize(value: impl serde::Serialize) -> Result<Value, ToolError> {
    serde_json::to_value(value).map_err(invalid)
}
fn repository(value: &str) -> Result<RepositoryId, ToolError> {
    RepositoryId::try_from(value.to_owned()).map_err(invalid)
}
fn revision(value: &str) -> Result<WorktreeRevision, ToolError> {
    value
        .split_once("+dirty.")
        .map_or_else(
            || WorktreeRevision::clean(value),
            |(base, dirty)| WorktreeRevision::dirty(base, dirty),
        )
        .map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> ToolError {
    ToolError::Invalid(error.to_string())
}
const fn default_budget() -> u32 {
    6_000
}
const fn default_results() -> usize {
    10
}
const fn default_trace_limit() -> usize {
    20
}
