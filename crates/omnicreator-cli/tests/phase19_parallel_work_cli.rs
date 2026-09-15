use std::{process::Command, str};

use omnicreator_application::{
    ApplicationControlService, ManualContentRequestV1, ProjectIdRequestV1,
};
use omnicreator_core::{
    initial_studio_pack_catalog_v1, ManualResultProvenanceV1, Workspace, WorkspaceSession,
};
use serde_json::{json, Value};

fn run(root: &std::path::Path, extra: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omnicreator"));
    command.arg("--data-root").arg(root).arg("--json");
    command.args(extra);
    command.output().unwrap()
}

fn seed_content(root: &std::path::Path) -> (String, String) {
    let workspace = Workspace::create(root).unwrap();
    let session = WorkspaceSession::acquire(workspace, "phase19-p2-cli-seed").unwrap();
    let mut service = ApplicationControlService::for_writer(&session).unwrap();
    let pack = initial_studio_pack_catalog_v1()
        .unwrap()
        .resolve_v1("christian-cinematic")
        .unwrap();
    let project = service
        .create_creator_project_v1("Phase 19 P2 CLI", &pack)
        .unwrap()
        .data
        .project;
    service
        .provide_manual_content_v1(&ManualContentRequestV1 {
            project_id: project.id.clone(),
            script: "CLI inspection must stay read-only while voice work can fan out from Content."
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
        .expect("Content should expose external voice work")
        .work_id
        .clone();
    drop(service);
    session.release().unwrap();
    (project.id, work_id)
}

#[test]
fn work_graph_and_prepare_are_read_only_and_commit_has_typed_rejection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data-root");
    let (project_id, work_id) = seed_content(&root);

    let graph = run(&root, &["work", "graph", "--project", project_id.as_str()]);
    assert!(
        graph.status.success(),
        "{}",
        str::from_utf8(&graph.stderr).unwrap()
    );
    let graph: Value = serde_json::from_slice(&graph.stdout).unwrap();
    assert_eq!(graph["ok"], true);
    assert_eq!(graph["operation"], "work.graph");
    assert_eq!(graph["data"]["schema"], "omnicreator.agent-work-graph");
    assert_eq!(graph["data"]["project_id"], project_id);
    assert_eq!(graph["data"]["read_only"], true);
    assert!(graph["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["work_id"] == work_id));

    let prepared = run(
        &root,
        &[
            "work",
            "prepare",
            "--project",
            project_id.as_str(),
            "--work",
            work_id.as_str(),
        ],
    );
    assert!(prepared.status.success());
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["data"]["work_id"], work_id);
    assert_eq!(prepared["data"]["external"]["kind"], "voice");
    let encoded = serde_json::to_string(&prepared).unwrap();
    assert!(!encoded.contains(root.to_string_lossy().as_ref()));

    let invalid_batch = temp.path().join("empty-batch.json");
    std::fs::write(
        &invalid_batch,
        serde_json::to_vec(&json!({
            "schema": "omnicreator.external-result-batch",
            "version": 1,
            "items": []
        }))
        .unwrap(),
    )
    .unwrap();
    let denied = run(
        &root,
        &[
            "--read-only",
            "work",
            "commit",
            "--input-file",
            invalid_batch.to_str().unwrap(),
        ],
    );
    assert_eq!(denied.status.code(), Some(4));
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["operation"], "work.commit");
    assert_eq!(denied["error"]["code"], "read_only");

    let invalid = run(
        &root,
        &[
            "work",
            "commit",
            "--input-file",
            invalid_batch.to_str().unwrap(),
        ],
    );
    assert_eq!(invalid.status.code(), Some(2));
    let invalid: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid["error"]["code"], "invalid_input");
}
