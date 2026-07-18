//! MCP dispatch for embedding models, living knowledge, workspaces, and
//! personal notes.
use pebble_core::service::PebbleService;
use serde::Deserialize;
use serde_json::Value;

use super::{ToolError, parse, repository, serialize};

pub(super) fn execute_plan2(
    service: &PebbleService,
    name: &str,
    value: Value,
) -> Result<Value, ToolError> {
    match name {
        "model_install" | "model_list" | "model_select" | "model_remove" | "note_list"
        | "note_read" | "update_list" | "update_apply" => execute_knowledge(service, name, value),
        _ => execute_workspace_personal(service, name, value),
    }
}

fn execute_knowledge(
    service: &PebbleService,
    name: &str,
    value: Value,
) -> Result<Value, ToolError> {
    match name {
        "model_install" => {
            let request: ModelInstallRequest = parse(value)?;
            crate::mcp_validation::model_install(&request.model_id)?;
            serialize(service.model_install(&request.model_id, request.confirm)?)
        }
        "model_list" => {
            let EmptyRequest {} = parse(value)?;
            serialize(service.model_list()?)
        }
        "model_select" => {
            let request: ModelIdRequest = parse(value)?;
            crate::mcp_validation::model_install(&request.model_id)?;
            serialize(service.model_select(&request.model_id)?)
        }
        "model_remove" => {
            let request: ModelIdRequest = parse(value)?;
            crate::mcp_validation::model_install(&request.model_id)?;
            serialize(service.model_remove(&request.model_id)?)
        }
        "note_list" => {
            let request: NoteListRequest = parse(value)?;
            crate::mcp_validation::id("repository", &request.repository)?;
            crate::mcp_validation::note_status(request.status.as_deref())?;
            serialize(
                service.note_list(&repository(&request.repository)?, request.status.as_deref())?,
            )
        }
        "note_read" => {
            let request: NoteReadRequest = parse(value)?;
            crate::mcp_validation::note_claim(&request.repository, &request.claim_id)?;
            serialize(service.note_read(&repository(&request.repository)?, &request.claim_id)?)
        }
        "update_list" => {
            let request: super::RepositoryRequest = parse(value)?;
            crate::mcp_validation::id("repository", &request.repository)?;
            serialize(service.update_list(&repository(&request.repository)?)?)
        }
        "update_apply" => {
            let request: UpdateApplyRequest = parse(value)?;
            crate::mcp_validation::update_apply(
                &request.repository,
                &request.claim_id,
                &request.patch,
            )?;
            serialize(service.update_apply(
                &repository(&request.repository)?,
                &request.claim_id,
                &request.patch,
            )?)
        }
        _ => Err(ToolError::Unknown),
    }
}

fn execute_workspace_personal(
    service: &PebbleService,
    name: &str,
    value: Value,
) -> Result<Value, ToolError> {
    match name {
        "workspace_create" => {
            let request: WorkspaceNameRequest = parse(value)?;
            crate::mcp_validation::workspace_name(&request.name)?;
            serialize(service.workspace_create(&request.name)?)
        }
        "workspace_add_repository" => {
            let request: WorkspaceAddRequest = parse(value)?;
            crate::mcp_validation::workspace_add_repository(&request.name, &request.repository)?;
            serialize(
                service
                    .workspace_add_repository(&request.name, &repository(&request.repository)?)?,
            )
        }
        "workspace_list" => {
            let EmptyRequest {} = parse(value)?;
            serialize(service.workspace_list()?)
        }
        "workspace_search" => {
            let request: WorkspaceSearchRequest = parse(value)?;
            crate::mcp_validation::workspace_search(
                &request.name,
                &request.query,
                request.budget_tokens,
                request.max_results,
            )?;
            serialize(service.workspace_search(
                &request.name,
                &request.query,
                request.budget_tokens,
                request.max_results,
            )?)
        }
        "personal_note_create" => {
            let request: PersonalCreateRequest = parse(value)?;
            crate::mcp_validation::personal_title(&request.title)?;
            serialize(service.personal_note_create(&request.title)?)
        }
        "personal_note_list" => {
            let EmptyRequest {} = parse(value)?;
            serialize(service.personal_note_list()?)
        }
        "personal_note_promote" => {
            let request: PersonalPromoteRequest = parse(value)?;
            crate::mcp_validation::personal_promote(&request.note_id, &request.repository)?;
            serialize(service.personal_note_promote(
                &request.note_id,
                &repository(&request.repository)?,
                request.confirm,
                request.acknowledge_overwrite,
            )?)
        }
        _ => Err(ToolError::Unknown),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyRequest {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelInstallRequest {
    model_id: String,
    #[serde(default)]
    confirm: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelIdRequest {
    model_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteListRequest {
    repository: String,
    status: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteReadRequest {
    repository: String,
    claim_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateApplyRequest {
    repository: String,
    claim_id: String,
    patch: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceNameRequest {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceAddRequest {
    name: String,
    repository: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSearchRequest {
    name: String,
    query: String,
    #[serde(default = "super::default_budget")]
    budget_tokens: u32,
    #[serde(default = "super::default_results")]
    max_results: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalCreateRequest {
    title: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalPromoteRequest {
    note_id: String,
    repository: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    acknowledge_overwrite: bool,
}
