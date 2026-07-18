//! End-to-end tool execution across the core and Plan 2 tool surfaces.
use super::*;

#[test]
fn every_tool_executes_through_the_service() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut server = fixture.server()?;
    server.initialize()?;
    let repository = fixture.repository.to_string_lossy();
    let initialized = server.call(2, "repository_init", json!({"repository": repository}))?;
    let repository_id = initialized["result"]["structuredContent"]["repository_id"]
        .as_str()
        .ok_or("repository id")?
        .to_owned();
    assert!(!repository_id.is_empty());
    assert!(!server
        .call(3, "repository_register", json!({"repository": repository}))?["result"]["isError"]
        .as_bool()
        .unwrap_or(true));
    git(&fixture.repository, &["add", ".pebble/pebble.toml"])?;
    git(&fixture.repository, &["commit", "-qm", "configure pebble"])?;
    assert!(!server
        .call(4, "repository_index", json!({"repository": repository}))?["result"]["isError"]
        .as_bool()
        .unwrap_or(true));
    let search = server.call(
        5,
        "search",
        json!({
            "query": "mcp_needle",
            "repository": repository_id,
            "budget_tokens": 1000,
            "max_results": 5
        }),
    )?;
    let packet = &search["result"]["structuredContent"];
    assert!(
        packet["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    let citation = &packet["items"][0]["citation"];
    let read = server.call(
        6,
        "evidence_read",
        json!({
            "repository": repository_id,
            "revision": revision_string(&citation["revision"])?,
            "path": citation["path"],
            "start_line": citation["start_line"],
            "end_line": citation["end_line"]
        }),
    )?;
    assert!(
        read["result"]["structuredContent"]["content"]
            .as_str()
            .is_some_and(|content| content.contains("mcp_needle"))
    );
    assert_eq!(
        server.call(7, "index_health", json!({"repository": repository_id}))?["result"]["structuredContent"]
            ["healthy"],
        true
    );
    assert!(
        server.call(
            8,
            "trace_list",
            json!({"repository": repository_id, "limit": 10}),
        )?["result"]["structuredContent"]
            .as_array()
            .is_some_and(|traces| !traces.is_empty())
    );
    assert!(!server
        .call(
            9,
            "projection_rebuild",
            json!({"repository": repository}),
        )?["result"]["isError"]
        .as_bool()
        .unwrap_or(true));
    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}

#[test]
fn plan2_tools_execute_through_the_service() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let repository_id = initialize_with_cli(&fixture)?;
    let mut server = fixture.server()?;
    server.initialize()?;

    let disclosure = server.call(
        2,
        "model_install",
        json!({"model_id": "all-minilm-l6-v2", "confirm": false}),
    )?;
    assert_eq!(
        disclosure["result"]["structuredContent"]["installed"],
        false
    );
    assert!(
        disclosure["result"]["structuredContent"]["disclosure"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    assert_eq!(
        server.call(3, "model_list", json!({}))?["result"]["structuredContent"],
        json!([])
    );

    assert_eq!(
        server.call(4, "note_list", json!({"repository": repository_id}))?["result"]["structuredContent"],
        json!([])
    );
    assert_eq!(
        server.call(5, "update_list", json!({"repository": repository_id}))?["result"]["structuredContent"],
        json!([])
    );

    let workspace = server.call(6, "workspace_create", json!({"name": "demo"}))?;
    assert_eq!(
        workspace["result"]["structuredContent"]["name"],
        json!("demo")
    );
    assert!(
        !server.call(
            7,
            "workspace_add_repository",
            json!({"name": "demo", "repository": repository_id}),
        )?["result"]["isError"]
            .as_bool()
            .unwrap_or(true)
    );
    assert_eq!(
        server.call(8, "workspace_list", json!({}))?["result"]["structuredContent"],
        json!(["demo"])
    );
    let workspace_search = server.call(
        9,
        "workspace_search",
        json!({"name": "demo", "query": "mcp_needle"}),
    )?;
    assert!(
        workspace_search["result"]["structuredContent"]["hits"]
            .as_array()
            .is_some_and(|hits| !hits.is_empty())
    );

    let note = server.call(10, "personal_note_create", json!({"title": "Sample"}))?;
    let note_id = note["result"]["structuredContent"]["id"]
        .as_str()
        .ok_or("note id")?
        .to_owned();
    assert!(
        server.call(11, "personal_note_list", json!({}))?["result"]["structuredContent"]
            .as_array()
            .is_some_and(|notes| notes.len() == 1)
    );
    let preview = server.call(
        12,
        "personal_note_promote",
        json!({"note_id": note_id, "repository": repository_id}),
    )?;
    assert_eq!(
        preview["result"]["structuredContent"]["applied"],
        json!(false)
    );
    assert!(
        preview["result"]["structuredContent"]["diff"]
            .as_str()
            .is_some_and(|diff| !diff.is_empty())
    );

    let (status, stderr) = server.close()?;
    assert!(status.success(), "{stderr}");
    Ok(())
}
