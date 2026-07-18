use super::*;

#[test]
fn malformed_calls_are_rejected_and_server_recovers() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    server.initialize()?;
    let malformed = server.call(2, "search", json!({"query": 7, "unexpected": true}))?;
    assert_eq!(malformed["error"]["code"], -32602);
    let unknown = server.call(3, "not_a_tool", json!({}))?;
    assert_eq!(unknown["error"]["code"], -32601);
    let ping = server.request(json!({"jsonrpc": "2.0", "id": 4, "method": "ping"}))?;
    assert!(ping["result"].is_object());
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

#[test]
fn every_schema_bound_is_enforced_before_service_dispatch() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    server.initialize()?;
    for (offset, (name, arguments)) in boundary_violation_cases().into_iter().enumerate() {
        let response = server.call(100 + u64::try_from(offset)?, name, arguments)?;
        assert_eq!(response["error"]["code"], -32602, "{name}: {response}");
    }
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

fn boundary_violation_cases() -> Vec<(&'static str, Value)> {
    let mut cases = core_boundary_cases();
    cases.extend(plan2_boundary_cases());
    cases
}

fn core_boundary_cases() -> Vec<(&'static str, Value)> {
    let valid_id = "repo";
    vec![
        ("repository_init", json!({"repository": ""})),
        (
            "repository_register",
            json!({"repository": "x", "extra": true}),
        ),
        ("repository_index", json!({"repository": "x".repeat(4097)})),
        ("index_health", json!({"repository": "x".repeat(257)})),
        ("trace_list", json!({"repository": valid_id, "limit": 0})),
        ("trace_list", json!({"repository": valid_id, "limit": 1001})),
        ("projection_rebuild", json!({"repository": "", "extra": 1})),
        ("search", json!({"query": "", "repository": valid_id})),
        (
            "search",
            json!({"query": "x".repeat(16385), "repository": valid_id}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "budget_tokens": 999}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "budget_tokens": 32001}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "max_results": 0}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "max_results": 101}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "revision": "x".repeat(257)}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "path_prefix": "x".repeat(4097)}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "language": "x".repeat(65)}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "kinds": ["chunk", "chunk"]}),
        ),
        (
            "search",
            json!({"query": "x", "repository": valid_id, "kinds": ["other"]}),
        ),
        (
            "evidence_read",
            json!({
                "repository": valid_id, "revision": "a", "path": "x".repeat(4097),
                "start_line": 1, "end_line": 1
            }),
        ),
        (
            "evidence_read",
            json!({
                "repository": valid_id, "revision": "a", "path": "x",
                "start_line": 0, "end_line": 1
            }),
        ),
    ]
}

fn plan2_boundary_cases() -> Vec<(&'static str, Value)> {
    let valid_id = "repo";
    vec![
        ("model_install", json!({"model_id": ""})),
        ("model_install", json!({"model_id": "x", "extra": true})),
        ("model_list", json!({"extra": true})),
        (
            "note_list",
            json!({"repository": valid_id, "status": "not_a_status"}),
        ),
        (
            "note_read",
            json!({"repository": valid_id, "claim_id": "x".repeat(257)}),
        ),
        (
            "update_apply",
            json!({"repository": valid_id, "claim_id": "c1", "patch": ""}),
        ),
        (
            "update_apply",
            json!({"repository": valid_id, "claim_id": "c1", "patch": "x".repeat(65_537)}),
        ),
        ("workspace_create", json!({"name": "not safe!"})),
        ("workspace_create", json!({"name": "x".repeat(129)})),
        (
            "workspace_search",
            json!({"name": "demo", "query": "x", "budget_tokens": 999}),
        ),
        (
            "workspace_search",
            json!({"name": "demo", "query": "x", "max_results": 0}),
        ),
        ("personal_note_create", json!({"title": ""})),
        ("personal_note_create", json!({"title": "x".repeat(513)})),
        (
            "personal_note_promote",
            json!({"note_id": "n1", "repository": valid_id, "extra": 1}),
        ),
    ]
}
