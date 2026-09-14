use std::{fs, path::Path, process::Stdio};

use omnicreator_application::{
    ApplicationControlService, ManualContentRequestV1, ManualScenePlanRequestV1,
    ManualVisualRequestV1, ManualVoiceRequestV1, ProjectIdRequestV1,
};
use omnicreator_core::{
    derive_manual_voice_timing_v1, ManualResultProvenanceV1, ManualScenePlanDraftV1, Workspace,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::process::Command;

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1280u32.to_be_bytes());
    bytes.extend_from_slice(&720u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 0, 1]);
    bytes
}

fn wav(duration_ms: u64) -> Vec<u8> {
    let sample_rate = 8_000u32;
    let channels = 1u16;
    let bits = 16u16;
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_size = ((u64::from(byte_rate) * duration_ms) / 1_000) as u32;
    let mut out = Vec::with_capacity((44 + data_size) as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend(std::iter::repeat_n(7u8, data_size as usize));
    out
}

fn structured(result: &CallToolResult) -> &Value {
    result
        .structured_content
        .as_ref()
        .expect("OmniCreator MCP tools must return structuredContent")
}

fn object(value: Value) -> serde_json::Map<String, Value> {
    value
        .as_object()
        .expect("tool arguments must be a JSON object")
        .clone()
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

fn mcp_transport(root: &Path, read_only: bool) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg("phase18-p2-mcp-test");
            if read_only {
                command.arg("--read-only");
            }
            command.arg("mcp").arg("serve");
            command.stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

fn payload<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

#[tokio::test]
async fn mcp_stdio_discovers_tools_and_full_manual_flow_matches_application_state() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    Workspace::create(&root).unwrap();

    let client = ().serve(mcp_transport(&root, false)).await.unwrap();
    let tools = client.list_tools(Default::default()).await.unwrap();
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for required in [
        "workspace_status",
        "projects_list",
        "project_get",
        "project_create",
        "project_update",
        "workflow_status",
        "workflow_set_step_auto",
        "creator_state",
        "creator_start_or_resume",
        "review_list",
        "content_control",
        "scene_plan_control",
        "visual_control",
        "voice_control",
        "production_control",
        "runtime_status",
    ] {
        assert!(
            names.contains(&required),
            "missing MCP tool {required}: {names:?}"
        );
    }

    let created = call(
        &client,
        "project_create",
        json!({
            "title": "Phase 18 P2 MCP manual flow",
            "studio_pack_id": "christian-cinematic"
        }),
    )
    .await;
    assert_eq!(created.is_error, Some(false));
    let project_id = structured(&created)["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let auto_off = call(
        &client,
        "workflow_set_step_auto",
        json!({"project_id": project_id, "step": "content.prepare", "enabled": false}),
    )
    .await;
    assert_eq!(auto_off.is_error, Some(false));
    assert_eq!(
        structured(&auto_off)["data"]["automatic_execution_enabled"],
        false
    );

    let script = "MCP must preserve the same canonical workflow truth as CLI and Desktop.";
    let content_request = ManualContentRequestV1 {
        project_id: project_id.clone(),
        script: script.to_owned(),
        provenance: ManualResultProvenanceV1::manual_editor(),
    };
    let content = call(
        &client,
        "content_control",
        json!({"action": "provide", "payload": payload(&content_request)}),
    )
    .await;
    assert_eq!(content.is_error, Some(false));
    let segment_id = structured(&content)["data"]["content"]["segments"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let policy = call(
        &client,
        "workflow_status",
        json!({"project_id": project_id}),
    )
    .await;
    let content_policy = structured(&policy)["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["step"] == "content.prepare")
        .expect("content policy");
    assert_eq!(content_policy["automatic_execution_enabled"], false);

    let editor = call(
        &client,
        "scene_plan_control",
        json!({"action": "editor", "project_id": project_id}),
    )
    .await;
    assert_eq!(editor.is_error, Some(false));
    let mut draft: ManualScenePlanDraftV1 =
        serde_json::from_value(structured(&editor)["data"][2].clone()).unwrap();
    draft.scenes[0].scene_type = "literal".to_owned();
    draft.scenes[0].purpose = "Prove MCP and application parity".to_owned();
    let scene_request = ManualScenePlanRequestV1 {
        project_id: project_id.clone(),
        draft,
        provenance: ManualResultProvenanceV1::manual_editor(),
    };
    let scene = call(
        &client,
        "scene_plan_control",
        json!({"action": "provide", "payload": payload(&scene_request)}),
    )
    .await;
    assert_eq!(scene.is_error, Some(false));
    let scene_id = structured(&scene)["data"]["scene_plan"]["scenes"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let visual_path = temp.path().join("mcp-visual.png");
    fs::write(&visual_path, png()).unwrap();
    let visual_request = ManualVisualRequestV1 {
        project_id: project_id.clone(),
        scene_id,
        source_path: visual_path,
        provenance: ManualResultProvenanceV1::local_file(),
        replace_existing: false,
    };
    let visual = call(
        &client,
        "visual_control",
        json!({"action": "provide_manual", "payload": payload(&visual_request)}),
    )
    .await;
    assert_eq!(visual.is_error, Some(false));

    let audio_path = temp.path().join("mcp-audio.wav");
    fs::write(&audio_path, wav(1_500)).unwrap();
    let voice_request = ManualVoiceRequestV1 {
        project_id: project_id.clone(),
        segment_id: segment_id.clone(),
        audio_path,
        timing: derive_manual_voice_timing_v1(&segment_id, script, 1_500).unwrap(),
        provenance: ManualResultProvenanceV1::local_file(),
        replace_existing: false,
    };
    let voice = call(
        &client,
        "voice_control",
        json!({"action": "provide_manual", "payload": payload(&voice_request)}),
    )
    .await;
    assert_eq!(voice.is_error, Some(false));

    let review = call(&client, "review_list", json!({"project_id": project_id})).await;
    assert_eq!(review.is_error, Some(false));
    assert_eq!(structured(&review)["operation"], "review_center");

    let recovery = call(
        &client,
        "production_control",
        json!({"action": "recovery", "project_id": project_id}),
    )
    .await;
    assert_eq!(recovery.is_error, Some(false));
    assert_eq!(
        structured(&recovery)["data"]["recovery"]["ready_for_rebuild"],
        true
    );

    let exported = call(
        &client,
        "production_control",
        json!({"action": "rebuild_export", "project_id": project_id}),
    )
    .await;
    assert_eq!(exported.is_error, Some(false));
    assert!(
        structured(&exported)["data"]["assembly"]["production_pack"]["tracks"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(structured(&exported)["data"]["export"]["artifacts"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));

    let project = call(&client, "project_get", json!({"project_id": project_id})).await;
    assert_eq!(project.is_error, Some(false));
    let encoded = serde_json::to_string(structured(&project)).unwrap();
    assert!(!encoded.contains(root.to_string_lossy().as_ref()));
    assert!(!encoded.contains("Bearer "));
    assert!(!encoded.contains("api_key"));

    let workspace = Workspace::inspect(&root).unwrap();
    let direct = ApplicationControlService::for_read_only(&workspace)
        .unwrap()
        .project_status_v1(&ProjectIdRequestV1 {
            project_id: project_id.clone(),
        })
        .unwrap();
    assert_eq!(*structured(&project), serde_json::to_value(direct).unwrap());

    client.cancel().await.unwrap();

    let read_only_client = ().serve(mcp_transport(&root, true)).await.unwrap();
    let denied = call(
        &read_only_client,
        "project_update",
        json!({
            "action": "rename",
            "project_id": project_id,
            "title": "must not persist"
        }),
    )
    .await;
    assert_eq!(denied.is_error, Some(true));
    assert_eq!(structured(&denied)["error"]["code"], "read_only");
    read_only_client.cancel().await.unwrap();
}

#[tokio::test]
async fn mcp_creator_start_reports_provider_unavailable_without_desktop_or_network_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    Workspace::create(&root).unwrap();
    let client = ().serve(mcp_transport(&root, false)).await.unwrap();

    let created = call(
        &client,
        "project_create",
        json!({
            "title": "MCP automatic blocker",
            "studio_pack_id": "christian-cinematic"
        }),
    )
    .await;
    let project_id = structured(&created)["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let result = call(
        &client,
        "creator_start_or_resume",
        json!({
            "project_id": project_id,
            "input": {"kind": "topic", "text": "A deterministic MCP provider blocker"}
        }),
    )
    .await;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(structured(&result)["error"]["code"], "provider_unavailable");
    let encoded = serde_json::to_string(structured(&result)).unwrap();
    assert!(!encoded.contains(root.to_string_lossy().as_ref()));

    client.cancel().await.unwrap();
}
