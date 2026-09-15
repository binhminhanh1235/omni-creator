use std::{path::Path, process::Stdio};

use omnicreator_application::{
    ApplicationControlService, ManualContentRequestV1, ProjectIdRequestV1,
};
use omnicreator_core::{
    initial_studio_pack_catalog_v1, ManualResultProvenanceV1, Workspace, WorkspaceSession,
};
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
        .expect("Phase 19 MCP tool must return structuredContent")
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: Value,
) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(object(arguments)))
        .await
        .unwrap()
}

fn transport(root: &Path, read_only: bool) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase19-p2-mcp-client");
            if read_only {
                command.arg("--read-only");
            }
            command.arg("mcp").arg("serve");
            command.stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

fn seed_content(root: &Path) -> (String, String) {
    let workspace = Workspace::create(root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p2-mcp-seed").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = service
        .create_creator_project_v1("Phase 19 P2 MCP", &pack)
        .unwrap()
        .data
        .project;
    service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script:
                "MCP harnesses should inspect canonical work without becoming canonical writers."
                    .to_owned(),
            provenance: ManualResultProvenanceV1::manual_editor(),
        })
        .unwrap();
    let graph = service
        .agent_work_graph_v1(&ProjectIdRequestV1 {
            project_id: project.id.clone(),
        })
        .unwrap();
    let work_id = graph
        .items
        .iter()
        .find(|item| item.external.is_some())
        .expect("verified Content should expose external voice work")
        .work_id
        .clone();
    drop(service);
    session.release().unwrap();
    (project.id, work_id)
}

#[tokio::test]
async fn real_stdio_client_discovers_phase19_tools_and_preserves_read_only_semantics() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    let (project_id, work_id) = seed_content(&root);

    let client = ().serve(transport(&root, true)).await.unwrap();
    let tools = client.list_tools(Default::default()).await.unwrap();
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for required in [
        "agent_work_graph",
        "agent_work_prepare",
        "agent_work_commit",
        "creator_start_or_resume",
    ] {
        assert!(
            names.contains(&required),
            "missing MCP tool {required}: {names:?}"
        );
    }

    let graph = call(
        &client,
        "agent_work_graph",
        json!({"project_id": project_id}),
    )
    .await;
    assert_eq!(graph.is_error, Some(false));
    assert_eq!(structured(&graph)["schema"], "omnicreator.agent-work-graph");
    assert_eq!(structured(&graph)["project_id"], project_id);
    assert_eq!(structured(&graph)["read_only"], true);

    let prepared = call(
        &client,
        "agent_work_prepare",
        json!({"project_id": project_id, "work_id": work_id}),
    )
    .await;
    assert_eq!(prepared.is_error, Some(false));
    assert_eq!(structured(&prepared)["work_id"], work_id);
    assert_eq!(structured(&prepared)["external"]["kind"], "voice");
    let encoded = serde_json::to_string(structured(&prepared)).unwrap();
    assert!(!encoded.contains(root.to_string_lossy().as_ref()));

    let denied = call(
        &client,
        "agent_work_commit",
        json!({
            "payload": {
                "schema": "omnicreator.external-result-batch",
                "version": 1,
                "items": []
            }
        }),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(structured(&denied)["error"]["code"], "read_only");
    client.cancel().await.unwrap();

    let writable = ().serve(transport(&root, false)).await.unwrap();
    let invalid = call(
        &writable,
        "agent_work_commit",
        json!({
            "payload": {
                "schema": "omnicreator.external-result-batch",
                "version": 1,
                "items": []
            }
        }),
    )
    .await;
    assert_eq!(invalid.is_error, Some(true));
    assert_eq!(structured(&invalid)["error"]["code"], "invalid_input");
    writable.cancel().await.unwrap();
}
