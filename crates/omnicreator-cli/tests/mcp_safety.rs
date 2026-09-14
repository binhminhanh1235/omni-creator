use std::{path::Path, process::Stdio};

use omnicreator_core::Workspace;
use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use tokio::process::Command;

fn object(value: Value) -> serde_json::Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be a JSON object")
        .clone()
}

fn structured(result: &CallToolResult) -> &Value {
    result
        .structured_content
        .as_ref()
        .expect("OmniCreator MCP tools must return structuredContent")
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(object(arguments)))
        .await
        .unwrap()
}

fn mcp_transport(root: &Path) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase18-p2-mcp-safety")
                .arg("mcp")
                .arg("serve")
                .stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn mcp_project_delete_requires_explicit_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    Workspace::create(&root).unwrap();

    let client = ().serve(mcp_transport(&root)).await.unwrap();
    let created = call(
        &client,
        "project_create",
        json!({"title": "Delete confirmation fixture"}),
    )
    .await;
    assert_eq!(created.is_error, Some(false));
    let project_id = structured(&created)["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let denied = call(
        &client,
        "project_update",
        json!({"action": "delete", "project_id": project_id}),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(structured(&denied)["error"]["code"], "invalid_input");
    assert_eq!(
        structured(&denied)["error"]["message"],
        "project delete requires confirm=true"
    );

    let still_present = call(
        &client,
        "project_get",
        json!({"project_id": project_id}),
    )
    .await;
    assert_eq!(still_present.is_error, Some(false));

    let deleted = call(
        &client,
        "project_update",
        json!({"action": "delete", "project_id": project_id, "confirm": true}),
    )
    .await;
    assert_eq!(deleted.is_error, Some(false));

    let missing = call(
        &client,
        "project_get",
        json!({"project_id": project_id}),
    )
    .await;
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(structured(&missing)["error"]["code"], "not_found");

    client.cancel().await.unwrap();
}
