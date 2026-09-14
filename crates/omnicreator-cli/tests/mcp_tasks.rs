use std::{fs, path::Path, process::Stdio, time::Duration};

use omnicreator_core::{
    deterministic_input_hash, ArtifactStore, StateStore, StepStatus, Workspace,
    CONTROL_TASK_CREATOR_START_OR_RESUME_V1,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
        ClientCapabilities, GetTaskParams, Implementation, InitializeRequestParams, TaskPayload,
        TaskStatus,
    },
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

fn mcp_transport(root: &Path, read_only: bool, device_id: &str) -> TokioChildProcess {
    TokioChildProcess::new(
        Command::new(env!("CARGO_BIN_EXE_omnicreator")).configure(|command| {
            command
                .arg("--data-root")
                .arg(root)
                .arg("--device-id")
                .arg(device_id);
            if read_only {
                command.arg("--read-only");
            }
            command.arg("mcp").arg("serve");
            command.stderr(Stdio::inherit());
        }),
    )
    .unwrap()
}

fn tasks_client_info() -> InitializeRequestParams {
    InitializeRequestParams::new(
        ClientCapabilities::builder().enable_tasks().build(),
        Implementation::from_build_env(),
    )
}

async fn create_project(root: &Path, title: &str) -> String {
    let client = ().serve(mcp_transport(root, false, "phase18-p5-project-setup")).await.unwrap();
    let result = client
        .call_tool(
            CallToolRequestParams::new("project_create").with_arguments(object(json!({
                "title": title
            }))),
        )
        .await
        .unwrap();
    let project_id = result
        .structured_content
        .as_ref()
        .expect("project_create structuredContent")["data"]["project"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    client.cancel().await.unwrap();
    project_id
}

async fn wait_for_terminal(
    client: &rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>,
    task_id: &str,
    poll_interval_ms: u64,
) -> rmcp::model::DetailedTask {
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(poll_interval_ms)).await;
        let task = client
            .peer()
            .get_task(GetTaskParams::new(task_id.to_owned()))
            .await
            .unwrap()
            .task;
        if task.status().is_terminal() {
            return task;
        }
    }
    panic!("task {task_id} did not reach a terminal state");
}

#[tokio::test]
async fn taskified_creator_result_survives_reconnect_and_data_root_move() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    Workspace::create(&root).unwrap();
    let project_id = create_project(&root, "P5 durable MCP task").await;

    let client = tasks_client_info()
        .serve(mcp_transport(&root, false, "phase18-p5-task-client-a"))
        .await
        .unwrap();
    let response = client
        .call_tool_once(
            CallToolRequestParams::new("creator_start_or_resume").with_arguments(object(json!({
                "project_id": project_id,
                "input": {
                    "kind": "topic",
                    "text": "A provider-free durable MCP task fixture"
                }
            }))),
        )
        .await
        .unwrap();
    let created = match response {
        CallToolResponse::Task(created) => created,
        other => panic!("expected MCP task response, got {other:?}"),
    };
    let task_id = created.task.task_id.clone();
    let terminal = wait_for_terminal(
        &client,
        &task_id,
        created.task.poll_interval_ms.unwrap_or(500),
    )
    .await;
    assert_eq!(terminal.status(), TaskStatus::Completed);
    let TaskPayload::Completed { result } = terminal.payload else {
        panic!("expected completed task payload");
    };
    let call_result: CallToolResult = serde_json::from_value(Value::Object(result)).unwrap();
    assert_eq!(call_result.is_error, Some(true));
    assert_eq!(
        call_result.structured_content.as_ref().unwrap()["error"]["code"],
        "provider_unavailable"
    );

    let workspace = Workspace::inspect(&root).unwrap();
    let store = StateStore::open_read_only(workspace.sqlite_path()).unwrap();
    let canonical = store.get_control_task_v1(&task_id).unwrap();
    assert_eq!(canonical.job.status, StepStatus::Succeeded);
    let artifact = canonical
        .selected_artifact
        .as_ref()
        .expect("completed task must select a canonical result artifact");
    assert_eq!(artifact.artifact_type, "control.task-result.v1");
    assert!(ArtifactStore::new(&root)
        .unwrap()
        .verify_artifact(artifact)
        .unwrap());
    drop(store);
    client.cancel().await.unwrap();

    let moved_root = temp.path().join("moved-data-root");
    fs::rename(&root, &moved_root).unwrap();
    let reconnected = tasks_client_info()
        .serve(mcp_transport(
            &moved_root,
            false,
            "phase18-p5-task-client-b",
        ))
        .await
        .unwrap();
    let recovered = reconnected
        .peer()
        .get_task(GetTaskParams::new(task_id.clone()))
        .await
        .unwrap()
        .task;
    assert_eq!(recovered.status(), TaskStatus::Completed);
    let TaskPayload::Completed { result } = recovered.payload else {
        panic!("moved Data Root must retain completed task payload");
    };
    let recovered_result: CallToolResult = serde_json::from_value(Value::Object(result)).unwrap();
    assert_eq!(recovered_result.is_error, Some(true));
    let encoded = serde_json::to_string(&recovered_result).unwrap();
    assert!(!encoded.contains(root.to_string_lossy().as_ref()));
    assert!(!encoded.contains(moved_root.to_string_lossy().as_ref()));
    assert!(!encoded.contains("Bearer "));
    assert!(!encoded.contains("api_key"));
    reconnected.cancel().await.unwrap();
}

#[tokio::test]
async fn tasks_cancel_is_canonical_and_restart_reconciles_interrupted_work_truthfully() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    let workspace = Workspace::create(&root).unwrap();
    let store = StateStore::open(workspace.sqlite_path()).unwrap();
    let project = store.create_project("P5 cancellation and restart").unwrap();
    drop(store);

    let client = tasks_client_info()
        .serve(mcp_transport(&root, false, "phase18-p5-cancel-client"))
        .await
        .unwrap();

    let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
    let cancel_hash = deterministic_input_hash(&[b"p5-cancel", project.id.as_bytes()]);
    let cancellable = store
        .create_control_task_v1(
            &project.id,
            CONTROL_TASK_CREATOR_START_OR_RESUME_V1,
            &cancel_hash,
            Some("mcp:test-cancel"),
        )
        .unwrap();
    let cancel_task_id = cancellable.job.job_id.clone();
    drop(store);

    client
        .peer()
        .cancel_task(CancelTaskParams::new(cancel_task_id.clone()))
        .await
        .unwrap();
    let cancelled = client
        .peer()
        .get_task(GetTaskParams::new(cancel_task_id.clone()))
        .await
        .unwrap()
        .task;
    assert_eq!(cancelled.status(), TaskStatus::Cancelled);

    let store = StateStore::open_read_only(workspace.sqlite_path()).unwrap();
    let canonical_cancelled = store.get_control_task_v1(&cancel_task_id).unwrap();
    assert_eq!(canonical_cancelled.job.status, StepStatus::Cancelled);
    assert_eq!(
        canonical_cancelled.attempts[0].status,
        StepStatus::Cancelled
    );
    assert_eq!(
        canonical_cancelled.attempts[0].error_code.as_deref(),
        Some("CONTROL_TASK_CANCELLED")
    );
    assert!(canonical_cancelled.selected_artifact.is_none());
    drop(store);

    let mut store = StateStore::open(workspace.sqlite_path()).unwrap();
    let restart_hash = deterministic_input_hash(&[b"p5-restart", project.id.as_bytes()]);
    let interrupted = store
        .create_control_task_v1(
            &project.id,
            CONTROL_TASK_CREATOR_START_OR_RESUME_V1,
            &restart_hash,
            Some("mcp:test-restart"),
        )
        .unwrap();
    let interrupted_task_id = interrupted.job.job_id.clone();
    drop(store);
    client.cancel().await.unwrap();

    let restarted = tasks_client_info()
        .serve(mcp_transport(&root, false, "phase18-p5-restart-client"))
        .await
        .unwrap();
    let reconciled = restarted
        .peer()
        .get_task(GetTaskParams::new(interrupted_task_id.clone()))
        .await
        .unwrap()
        .task;
    assert_eq!(reconciled.status(), TaskStatus::Failed);
    let TaskPayload::Failed { error } = reconciled.payload else {
        panic!(
            "interrupted canonical task must be reported as failed after restart reconciliation"
        );
    };
    assert_eq!(error["code"], "canonical_task_interrupted");

    let store = StateStore::open_read_only(workspace.sqlite_path()).unwrap();
    let canonical_reconciled = store.get_control_task_v1(&interrupted_task_id).unwrap();
    assert_eq!(canonical_reconciled.job.status, StepStatus::Retryable);
    assert_eq!(
        canonical_reconciled.attempts[0].status,
        StepStatus::Retryable
    );
    assert_eq!(
        canonical_reconciled.attempts[0].error_code.as_deref(),
        Some("LOCAL_RESTART_PENDING_RECONCILIATION")
    );
    drop(store);
    restarted.cancel().await.unwrap();

    let read_only = tasks_client_info()
        .serve(mcp_transport(&root, true, "phase18-p5-read-only-client"))
        .await
        .unwrap();
    let denied = read_only
        .peer()
        .cancel_task(CancelTaskParams::new(cancel_task_id))
        .await
        .expect_err("read-only task cancellation must be rejected");
    let encoded = format!("{denied:?}");
    assert!(encoded.contains("read-only"));
    read_only.cancel().await.unwrap();
}
