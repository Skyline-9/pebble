//! Closed JSON schemas for every Pebble MCP tool.

use std::sync::Arc;

use rmcp::model::{JsonObject, Tool};
use serde_json::{Value, json};

use crate::mcp_tools::SEARCH_TOOL;

pub fn tools() -> Vec<Tool> {
    let mut tools = core_tools();
    tools.extend(plan2_tools());
    tools
}

fn core_tools() -> Vec<Tool> {
    vec![
        tool(
            "repository_init",
            "Initialize portable Pebble configuration for a repository.",
            path_schema(),
        ),
        tool(
            "repository_register",
            "Register one initialized local checkout.",
            object(
                &json!({
                    "repository": path_property(),
                    "alternate_worktree": {"type": "boolean", "default": false}
                }),
                &["repository"],
            ),
        ),
        tool(
            "repository_index",
            "Compile and atomically activate an immutable repository index.",
            path_schema(),
        ),
        tool(
            SEARCH_TOOL,
            "Search model-free repository evidence within a strict token budget.",
            search_schema(),
        ),
        tool(
            "evidence_read",
            "Resolve an exact citation against its indexed worktree revision.",
            read_schema(),
        ),
        tool(
            "index_health",
            "Validate the current immutable index generation.",
            repository_schema(),
        ),
        tool(
            "trace_list",
            "List a bounded tail of local retrieval traces.",
            object(
                &json!({
                    "repository": id_property(),
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 20}
                }),
                &["repository"],
            ),
        ),
        tool(
            "projection_rebuild",
            "Build and atomically activate a fresh disposable projection.",
            path_schema(),
        ),
    ]
}

fn plan2_tools() -> Vec<Tool> {
    let mut tools = model_tools();
    tools.extend(knowledge_tools());
    tools.extend(workspace_tools());
    tools.extend(personal_tools());
    tools
}

fn model_tools() -> Vec<Tool> {
    vec![
        tool(
            "model_install",
            "Show the consent disclosure for, or install, a local embedding model.",
            object(
                &json!({
                    "model_id": id_property(),
                    "confirm": {"type": "boolean", "default": false}
                }),
                &["model_id"],
            ),
        ),
        tool(
            "model_list",
            "List every installed local embedding model.",
            object(&json!({}), &[]),
        ),
        tool(
            "model_select",
            "Select the active local embedding model for model-augmented search.",
            object(&json!({"model_id": id_property()}), &["model_id"]),
        ),
        tool(
            "model_remove",
            "Remove one installed local embedding model.",
            object(&json!({"model_id": id_property()}), &["model_id"]),
        ),
    ]
}

fn knowledge_tools() -> Vec<Tool> {
    vec![
        tool(
            "note_list",
            "List managed living-knowledge claims for a registered repository.",
            object(
                &json!({
                    "repository": id_property(),
                    "status": {"enum": ["current", "stale", "pending_update", "broken"]}
                }),
                &["repository"],
            ),
        ),
        tool(
            "note_read",
            "Read one managed living-knowledge claim's current status and prose.",
            object(
                &json!({"repository": id_property(), "claim_id": id_property()}),
                &["repository", "claim_id"],
            ),
        ),
        tool(
            "update_list",
            "List queued living-note update packets awaiting a replacement patch.",
            repository_schema(),
        ),
        tool(
            "update_apply",
            "Validate and apply one queued replacement patch to a claim's managed region.",
            object(
                &json!({
                    "repository": id_property(),
                    "claim_id": id_property(),
                    "patch": {"type": "string", "minLength": 1, "maxLength": 65536}
                }),
                &["repository", "claim_id", "patch"],
            ),
        ),
    ]
}

fn workspace_tools() -> Vec<Tool> {
    vec![
        tool(
            "workspace_create",
            "Create a new empty multi-repository workspace.",
            object(&json!({"name": workspace_name_property()}), &["name"]),
        ),
        tool(
            "workspace_add_repository",
            "Add one registered repository to a workspace.",
            object(
                &json!({"name": workspace_name_property(), "repository": id_property()}),
                &["name", "repository"],
            ),
        ),
        tool(
            "workspace_list",
            "List every workspace's name.",
            object(&json!({}), &[]),
        ),
        tool(
            "workspace_search",
            "Search every present repository in a workspace and merge results by score.",
            object(
                &json!({
                    "name": workspace_name_property(),
                    "query": {"type": "string", "minLength": 1, "maxLength": 16384},
                    "budget_tokens": {
                        "type": "integer", "minimum": 1000, "maximum": 32000, "default": 6000
                    },
                    "max_results": {
                        "type": "integer", "minimum": 1, "maximum": 100, "default": 10
                    }
                }),
                &["name", "query"],
            ),
        ),
    ]
}

fn personal_tools() -> Vec<Tool> {
    vec![
        tool(
            "personal_note_create",
            "Create a new personal knowledge note stored outside any repository.",
            object(
                &json!({"title": {"type": "string", "minLength": 1, "maxLength": 512}}),
                &["title"],
            ),
        ),
        tool(
            "personal_note_list",
            "List every personal knowledge note.",
            object(&json!({}), &[]),
        ),
        tool(
            "personal_note_promote",
            "Preview or apply promoting one personal note into a repository's shared knowledge.",
            object(
                &json!({
                    "note_id": id_property(),
                    "repository": id_property(),
                    "confirm": {"type": "boolean", "default": false},
                    "acknowledge_overwrite": {"type": "boolean", "default": false}
                }),
                &["note_id", "repository"],
            ),
        ),
    ]
}

fn tool(name: &'static str, description: &'static str, schema: JsonObject) -> Tool {
    Tool::new(name, description, Arc::new(schema))
}

fn path_schema() -> JsonObject {
    object(&json!({"repository": path_property()}), &["repository"])
}

fn repository_schema() -> JsonObject {
    object(&json!({"repository": id_property()}), &["repository"])
}

fn search_schema() -> JsonObject {
    object(
        &json!({
            "query": {"type": "string", "minLength": 1, "maxLength": 16384},
            "repository": id_property(),
            "budget_tokens": {
                "type": "integer", "minimum": 1000, "maximum": 32000, "default": 6000
            },
            "max_results": {
                "type": "integer", "minimum": 1, "maximum": 100, "default": 10
            },
            "revision": {"type": "string", "minLength": 1, "maxLength": 256},
            "path_prefix": {"type": "string", "minLength": 1, "maxLength": 4096},
            "language": {"type": "string", "minLength": 1, "maxLength": 64},
            "kinds": {
                "type": "array", "items": {"enum": ["chunk", "symbol", "file"]},
                "uniqueItems": true, "maxItems": 3
            }
        }),
        &["query", "repository"],
    )
}

fn read_schema() -> JsonObject {
    object(
        &json!({
            "repository": id_property(),
            "revision": {"type": "string", "minLength": 1, "maxLength": 256},
            "path": {"type": "string", "minLength": 1, "maxLength": 4096},
            "start_line": {"type": "integer", "minimum": 1},
            "end_line": {"type": "integer", "minimum": 1}
        }),
        &["repository", "revision", "path", "start_line", "end_line"],
    )
}

fn path_property() -> Value {
    json!({"type": "string", "minLength": 1, "maxLength": 4096})
}

fn id_property() -> Value {
    json!({
        "type": "string", "minLength": 1, "maxLength": 256,
        "pattern": "^[A-Za-z0-9._-]+$"
    })
}

fn workspace_name_property() -> Value {
    json!({
        "type": "string", "minLength": 1, "maxLength": 128,
        "pattern": "^[A-Za-z0-9._-]+$"
    })
}

fn object(properties: &Value, required: &[&str]) -> JsonObject {
    rmcp::model::object(json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    }))
}
